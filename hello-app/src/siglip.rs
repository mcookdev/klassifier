//! Zero-shot image classifier using SigLIP (google/siglip-base-patch16-224, ONNX export by
//! Xenova). Each label gets an independent score from 0 to 1, so several labels can match one
//! image. Runs on the GPU through DirectML when available, otherwise on the CPU.
//!
//! Label embeddings come from the text model, which is only needed when the labels change:
//! they are cached in a file keyed by the labels and model files.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, ensure, Context, Result};
use image::imageops::FilterType;
use image::DynamicImage;
use ndarray::{Array2, Array4, ArrayView1, Ix2};
use ort::ep;
use ort::session::Session;
use ort::value::TensorRef;
use tokenizers::{PaddingParams, PaddingStrategy, Tokenizer, TruncationParams};

/// Model input size (preprocessor_config.json).
pub const IMAGE_SIZE: u32 = 224;
/// SigLIP's text encoder expects exactly 64 tokens, padded with `</s>` (id 1).
const TEXT_TOKENS: usize = 64;
const PAD_ID: u32 = 1;
const PAD_TOKEN: &str = "</s>";
/// exp(logit_scale) and logit_bias, read from google/siglip-base-patch16-224's weights.
const LOGIT_SCALE: f32 = 117.330_77;
const LOGIT_BIAS: f32 = -12.932_437;
const TEXT_BATCH: usize = 32;
/// Previews over `PRESHRINK_ABOVE` are shrunk quickly to `PRESHRINK_EDGE` before the final
/// resize (see `preprocess`). Only reductions of 2x or more, where area averaging is accurate.
const PRESHRINK_ABOVE: u32 = 8 * IMAGE_SIZE;
const PRESHRINK_EDGE: u32 = 4 * IMAGE_SIZE;
/// Bump when anything that affects cached label embeddings changes (prompt, tokenizing, ...).
const CACHE_VERSION: u32 = 1;

#[cfg(windows)]
const ORT_LIBRARY: &str = "onnxruntime.dll";
#[cfg(not(windows))]
const ORT_LIBRARY: &str = "libonnxruntime.so";

/// The model files, as laid out by `make fetch`.
pub struct ModelFiles {
    pub vision: PathBuf,
    pub text: PathBuf,
    pub tokenizer: PathBuf,
}

impl ModelFiles {
    pub fn in_dir(dir: &Path) -> Self {
        ModelFiles {
            vision: dir.join("vision_model.onnx"),
            text: dir.join("text_model.onnx"),
            tokenizer: dir.join("tokenizer.json"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    DirectML,
    Cpu,
}

impl Backend {
    pub fn as_str(self) -> &'static str {
        match self {
            Backend::DirectML => "DirectML",
            Backend::Cpu => "CPU",
        }
    }
}

/// Loads ONNX Runtime from `dir`. Call once, before creating a `Classifier`.
pub fn init_runtime(dir: &Path) -> Result<()> {
    let directml = dir.join("DirectML.dll");
    if directml.is_file() {
        // Load ours before ONNX Runtime asks for it, so an older copy in System32 isn't used.
        ort::util::preload_dylib(&directml)
            .map_err(|e| anyhow!("cannot load {}: {e}", directml.display()))?;
    }
    let library = dir.join(ORT_LIBRARY);
    ensure!(library.is_file(), "{} not found (run `make fetch` and `make deploy`)", library.display());
    ort::init_from(&library)
        .map_err(|e| anyhow!("cannot load {}: {e}", library.display()))?
        .commit();
    Ok(())
}

pub struct Classifier {
    vision: Session,
    backend: Backend,
    labels: Vec<String>,
    /// One L2-normalized row per label.
    label_embeddings: Array2<f32>,
    labels_from_cache: bool,
}

impl Classifier {
    /// Loads the vision model and the label embeddings (from `cache_dir` when possible).
    pub fn new(files: &ModelFiles, labels: Vec<String>, cache_dir: &Path, use_gpu: bool) -> Result<Self> {
        ensure!(!labels.is_empty(), "no labels to classify with");
        let (vision, backend) = open_session(&files.vision, use_gpu)?;
        check_io(&vision, "pixel_values")?;

        let cache = cache_dir.join(format!("hello-app-siglip-{:016x}.bin", cache_key(files, &labels)?));
        let (label_embeddings, labels_from_cache) = match read_cache(&cache, labels.len()) {
            Some(embeddings) => (embeddings, true),
            None => {
                let embeddings = embed_labels(files, &labels, use_gpu)?;
                if let Err(e) = write_cache(&cache, &embeddings) {
                    eprintln!("warning: cannot write {}: {e:#}", cache.display());
                }
                (embeddings, false)
            }
        };
        Ok(Classifier { vision, backend, labels, label_embeddings, labels_from_cache })
    }

    pub fn backend(&self) -> Backend {
        self.backend
    }

    pub fn labels_from_cache(&self) -> bool {
        self.labels_from_cache
    }

    pub fn label_count(&self) -> usize {
        self.labels.len()
    }

    /// Scores every label for `image` and returns the `top` best, highest first.
    pub fn classify(&mut self, image: &DynamicImage, top: usize) -> Result<Vec<(String, f32)>> {
        let pixels = preprocess(image);
        let outputs = self
            .vision
            .run(ort::inputs!["pixel_values" => TensorRef::from_array_view(pixels.view()).map_err(ort_error)?])
            .map_err(ort_error)?;
        let embedding = outputs["pooler_output"]
            .try_extract_array::<f32>()
            .map_err(ort_error)?
            .into_dimensionality::<Ix2>()?;
        let embedding = normalized(embedding.row(0));

        let similarities = self.label_embeddings.dot(&embedding);
        let mut scores: Vec<(usize, f32)> = similarities
            .iter()
            .enumerate()
            .map(|(i, &cos)| (i, sigmoid(LOGIT_SCALE * cos + LOGIT_BIAS)))
            .collect();
        scores.sort_by(|a, b| b.1.total_cmp(&a.1));
        Ok(scores.into_iter().take(top).map(|(i, score)| (self.labels[i].clone(), score)).collect())
    }
}

/// Reads a labels file: one label per line; blank lines and `#` comments are skipped, and
/// duplicates are dropped.
pub fn read_labels(path: &Path) -> Result<Vec<String>> {
    let text = fs::read_to_string(path).with_context(|| format!("cannot read labels file {}", path.display()))?;
    let mut labels: Vec<String> = Vec::new();
    for line in text.lines().map(str::trim) {
        if !line.is_empty() && !line.starts_with('#') && !labels.iter().any(|l| l == line) {
            labels.push(line.to_string());
        }
    }
    ensure!(!labels.is_empty(), "labels file {} has no labels", path.display());
    Ok(labels)
}

/// The text the model sees for a label. SigLIP was trained on lowercased text with
/// punctuation removed, which its Hugging Face tokenizer does but `tokenizer.json` doesn't.
pub fn prompt(label: &str) -> String {
    let text = format!("a photo of {label}.");
    let text: String = text
        .chars()
        .filter(|c| !c.is_ascii_punctuation())
        .flat_map(char::to_lowercase)
        .collect();
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Resizes straight to 224x224 (no crop) with a bicubic filter, scales to [-1, 1], NCHW.
pub fn preprocess(image: &DynamicImage) -> Array4<f32> {
    let size = IMAGE_SIZE as usize;
    // A bicubic resize costs time in proportion to the source size, so shrink large previews
    // (an 18 MP CR2 preview, say) with a fast area average first: about 5x faster.
    let shrunk;
    let image = if image.width().max(image.height()) > PRESHRINK_ABOVE {
        shrunk = image.thumbnail(PRESHRINK_EDGE, PRESHRINK_EDGE);
        &shrunk
    } else {
        image
    };
    let rgb = image.resize_exact(IMAGE_SIZE, IMAGE_SIZE, FilterType::CatmullRom).to_rgb8();
    let mut pixels = Array4::<f32>::zeros((1, 3, size, size));
    for (x, y, pixel) in rgb.enumerate_pixels() {
        for c in 0..3 {
            pixels[[0, c, y as usize, x as usize]] = pixel[c] as f32 / 127.5 - 1.0;
        }
    }
    pixels
}

fn embed_labels(files: &ModelFiles, labels: &[String], use_gpu: bool) -> Result<Array2<f32>> {
    let mut tokenizer = Tokenizer::from_file(&files.tokenizer)
        .map_err(|e| anyhow!("cannot load {}: {e}", files.tokenizer.display()))?;
    tokenizer.with_padding(Some(PaddingParams {
        strategy: PaddingStrategy::Fixed(TEXT_TOKENS),
        pad_id: PAD_ID,
        pad_token: PAD_TOKEN.into(),
        ..Default::default()
    }));
    tokenizer
        .with_truncation(Some(TruncationParams { max_length: TEXT_TOKENS, ..Default::default() }))
        .map_err(|e| anyhow!("cannot configure tokenizer: {e}"))?;

    let (mut text_model, _) = open_session(&files.text, use_gpu)?;
    check_io(&text_model, "input_ids")?;

    let mut rows: Vec<f32> = Vec::new();
    let mut dim = 0;
    for batch in labels.chunks(TEXT_BATCH) {
        let prompts: Vec<String> = batch.iter().map(|l| prompt(l)).collect();
        let encodings = tokenizer.encode_batch(prompts, true).map_err(|e| anyhow!("cannot tokenize labels: {e}"))?;
        let ids: Vec<i64> = encodings.iter().flat_map(|e| e.get_ids().iter().map(|&id| id as i64)).collect();
        let ids = Array2::from_shape_vec((batch.len(), TEXT_TOKENS), ids)?;

        let outputs = text_model
            .run(ort::inputs!["input_ids" => TensorRef::from_array_view(ids.view()).map_err(ort_error)?])
            .map_err(ort_error)?;
        let embeddings = outputs["pooler_output"]
            .try_extract_array::<f32>()
            .map_err(ort_error)?
            .into_dimensionality::<Ix2>()?;
        dim = embeddings.ncols();
        for row in embeddings.rows() {
            rows.extend(normalized(row));
        }
    }
    Ok(Array2::from_shape_vec((labels.len(), dim), rows)?)
}

fn open_session(model: &Path, use_gpu: bool) -> Result<(Session, Backend)> {
    ensure!(model.is_file(), "model file {} not found (run `make fetch` and `make deploy`)", model.display());
    if use_gpu {
        match build_session(model, true) {
            Ok(session) => return Ok((session, Backend::DirectML)),
            Err(e) => eprintln!("warning: DirectML unavailable, using the CPU: {e:#}"),
        }
    }
    let session = build_session(model, false).with_context(|| format!("cannot load {}", model.display()))?;
    Ok((session, Backend::Cpu))
}

fn build_session(model: &Path, gpu: bool) -> Result<Session> {
    let mut builder = Session::builder().map_err(ort_error)?;
    if gpu {
        // DirectML doesn't support memory patterns or parallel execution.
        builder = builder
            .with_memory_pattern(false)
            .map_err(ort_error)?
            .with_parallel_execution(false)
            .map_err(ort_error)?
            .with_execution_providers([ep::DirectML::default().build().error_on_failure()])
            .map_err(ort_error)?;
    }
    builder.commit_from_file(model).map_err(ort_error)
}

/// Fails with a clear message if the model doesn't have the expected input/output names.
fn check_io(session: &Session, input: &str) -> Result<()> {
    let inputs: Vec<&str> = session.inputs().iter().map(|o| o.name()).collect();
    let outputs: Vec<&str> = session.outputs().iter().map(|o| o.name()).collect();
    ensure!(
        inputs.contains(&input) && outputs.contains(&"pooler_output"),
        "unexpected model: inputs {inputs:?}, outputs {outputs:?} (expected {input} -> pooler_output)"
    );
    Ok(())
}

fn ort_error(e: impl std::fmt::Display) -> anyhow::Error {
    anyhow!("ONNX Runtime: {e}")
}

fn normalized(v: ArrayView1<f32>) -> ndarray::Array1<f32> {
    let norm = v.dot(&v).sqrt().max(f32::EPSILON);
    v.mapv(|x| x / norm)
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

// ---- label embedding cache ------------------------------------------------------------

const CACHE_MAGIC: &[u8; 4] = b"HSL1";

/// FNV-1a over everything the label embeddings depend on. (Stable across builds, unlike
/// `DefaultHasher`.)
fn cache_key(files: &ModelFiles, labels: &[String]) -> Result<u64> {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for &b in bytes {
            hash = (hash ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    feed(&CACHE_VERSION.to_le_bytes());
    for path in [&files.text, &files.tokenizer] {
        let len = fs::metadata(path).with_context(|| format!("cannot read {}", path.display()))?.len();
        feed(&len.to_le_bytes());
    }
    for label in labels {
        feed(prompt(label).as_bytes());
        feed(b"\n");
    }
    Ok(hash)
}

fn read_cache(path: &Path, rows: usize) -> Option<Array2<f32>> {
    let bytes = fs::read(path).ok()?;
    let (magic, rest) = bytes.split_at_checked(4)?;
    let (header, data) = rest.split_at_checked(8)?;
    let cached_rows = u32::from_le_bytes(header[..4].try_into().ok()?) as usize;
    let dim = u32::from_le_bytes(header[4..].try_into().ok()?) as usize;
    if magic != CACHE_MAGIC || cached_rows != rows || data.len() != rows * dim * 4 {
        return None;
    }
    let values = data.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
    Array2::from_shape_vec((rows, dim), values).ok()
}

fn write_cache(path: &Path, embeddings: &Array2<f32>) -> Result<()> {
    let mut bytes = Vec::with_capacity(12 + embeddings.len() * 4);
    bytes.extend_from_slice(CACHE_MAGIC);
    bytes.extend_from_slice(&(embeddings.nrows() as u32).to_le_bytes());
    bytes.extend_from_slice(&(embeddings.ncols() as u32).to_le_bytes());
    for v in embeddings.iter() {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    // Write then rename, so a concurrent run never reads a half-written file.
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    fs::write(&tmp, &bytes)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_is_lowercased_without_punctuation() {
        assert_eq!(prompt("Close-Up"), "a photo of closeup");
        assert_eq!(prompt("  black   and white "), "a photo of black and white");
    }

    #[test]
    fn preprocess_scales_to_minus_one_one() {
        let white = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(500, 300, image::Rgb([255, 255, 255])));
        let pixels = preprocess(&white);
        assert_eq!(pixels.shape(), &[1, 3, 224, 224]);
        assert!(pixels.iter().all(|&v| (v - 1.0).abs() < 1e-6));
    }

    #[test]
    fn cache_round_trips() {
        let dir = std::env::temp_dir().join(format!("hello-app-cache-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("c.bin");
        let e = Array2::from_shape_vec((2, 3), vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]).unwrap();
        write_cache(&path, &e).unwrap();
        assert_eq!(read_cache(&path, 2), Some(e));
        assert_eq!(read_cache(&path, 3), None);
        fs::remove_dir_all(&dir).unwrap();
    }
}
