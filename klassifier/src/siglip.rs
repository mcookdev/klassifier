//! Zero-shot image classifier using SigLIP (google/siglip-base-patch16-224, ONNX export by
//! Xenova). Each label gets an independent score from 0 to 1, so several labels can match one
//! image. Runs on the GPU through DirectML when available, otherwise on the CPU. With more
//! than one GPU (a desktop CPU's integrated graphics and a graphics card, say), DirectML is
//! asked for the one Windows ranks as high-performance.
//!
//! Label embeddings come from the text model, which is only needed when the labels change:
//! they are cached in a file keyed by the labels and model files.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, ensure, Context, Result};
use image::imageops::FilterType;
use image::DynamicImage;
use ndarray::{Array2, Array4, ArrayView1, Ix2};
use ort::environment::Environment;
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
/// Match levels by the best raw score in a photo (see `MatchLevel`).
const STRONG_MATCH: f32 = 0.10;
const GOOD_MATCH: f32 = 0.01;
/// Default raw-score cutoff for suggesting a label as a keyword (see `suggested`).
pub const SUGGEST_MIN: f32 = GOOD_MATCH;
/// Most keywords suggested per photo.
pub const SUGGEST_MAX: usize = 5;
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

/// Loads ONNX Runtime from `dir`. Call once; the `Classifier` is created in the environment returned.
pub fn init_runtime(dir: &Path) -> Result<Environment> {
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
        .build()
        .map_err(ort_error)
}

/// One label's result for a photo.
#[derive(Debug, Clone, PartialEq)]
pub struct LabelScore {
    pub label: String,
    /// SigLIP's own score: the chance this label applies, judged on its own. Low in absolute
    /// terms (a clear match is often 0.05-0.3), but comparable across photos, so it's what a
    /// keyword cutoff should use.
    pub score: f32,
    /// This label's share of all labels for the photo (softmax of the logits; all labels sum to
    /// 1). Easier to read, but some label always gets a large share, even if nothing matches.
    pub share: f32,
}

/// How much to trust a photo's labels, from its best raw score.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchLevel {
    Strong,
    Good,
    Weak,
}

impl MatchLevel {
    pub fn from_best_score(score: f32) -> Self {
        if score >= STRONG_MATCH {
            MatchLevel::Strong
        } else if score >= GOOD_MATCH {
            MatchLevel::Good
        } else {
            MatchLevel::Weak
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            MatchLevel::Strong => "strong",
            MatchLevel::Good => "good",
            MatchLevel::Weak => "weak",
        }
    }
}

pub struct Classifier {
    vision: Session,
    backend: Backend,
    gpu_name: Option<String>,
    labels: Vec<String>,
    /// One L2-normalized row per label.
    label_embeddings: Array2<f32>,
    labels_from_cache: bool,
}

impl Classifier {
    /// Loads the vision model and the label embeddings (from `cache_dir` when possible).
    pub fn new(env: &Environment, files: &ModelFiles, labels: Vec<String>, cache_dir: &Path, use_gpu: bool) -> Result<Self> {
        ensure!(!labels.is_empty(), "no labels to classify with");
        let (vision, backend) = open_session(env, &files.vision, use_gpu)?;
        check_io(&vision, "pixel_values")?;

        let cache = cache_dir.join(format!("klassifier-siglip-{:016x}.bin", cache_key(files, &labels)?));
        let (label_embeddings, labels_from_cache) = match read_cache(&cache, labels.len()) {
            Some(embeddings) => (embeddings, true),
            None => {
                let embeddings = embed_labels(env, files, &labels, use_gpu)?;
                if let Err(e) = write_cache(&cache, &embeddings) {
                    eprintln!("warning: cannot write {}: {e:#}", cache.display());
                }
                (embeddings, false)
            }
        };
        let gpu_name = if backend == Backend::DirectML { high_performance_gpu_name() } else { None };
        Ok(Classifier { vision, backend, gpu_name, labels, label_embeddings, labels_from_cache })
    }

    pub fn backend(&self) -> Backend {
        self.backend
    }

    /// The GPU's name when running on DirectML, if Windows reports one.
    pub fn gpu_name(&self) -> Option<&str> {
        self.gpu_name.as_deref()
    }

    pub fn labels_from_cache(&self) -> bool {
        self.labels_from_cache
    }

    pub fn label_count(&self) -> usize {
        self.labels.len()
    }

    /// Scores every label for `image` and returns the `top` best, highest first.
    pub fn classify(&mut self, image: &DynamicImage, top: usize) -> Result<Vec<LabelScore>> {
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

        let logits: Vec<f32> =
            self.label_embeddings.dot(&embedding).iter().map(|&cos| LOGIT_SCALE * cos + LOGIT_BIAS).collect();
        Ok(rank(&self.labels, &logits, top))
    }
}

/// Turns per-label logits into the `top` best `LabelScore`s, highest first.
fn rank(labels: &[String], logits: &[f32], top: usize) -> Vec<LabelScore> {
    // Softmax, shifted by the max logit so exp() can't overflow.
    let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let exps: Vec<f32> = logits.iter().map(|&l| (l - max).exp()).collect();
    let total: f32 = exps.iter().sum();

    let mut order: Vec<usize> = (0..logits.len()).collect();
    order.sort_by(|&a, &b| logits[b].total_cmp(&logits[a]));
    order
        .into_iter()
        .take(top)
        .map(|i| LabelScore { label: labels[i].clone(), score: sigmoid(logits[i]), share: exps[i] / total })
        .collect()
}

/// The labels to suggest as keywords: the leading labels of `ranked` (best first, as from
/// `Classifier::classify`) whose raw score is at least `min`, at most `max` of them.
pub fn suggested(ranked: &[LabelScore], min: f32, max: usize) -> &[LabelScore] {
    let passing = ranked.iter().take_while(|l| l.score >= min).count();
    &ranked[..passing.min(max)]
}

/// Labels read from labels files, with the Lightroom keyword each one maps to, if any.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LabelSet {
    /// What the model scores, in file order. Never includes the `=> ...` part of a line.
    pub labels: Vec<String>,
    /// Label (as in `labels`) -> keyword path from the top level down, e.g. `["Wildlife", "Bird"]`.
    /// Labels without a mapping aren't in here.
    pub keywords: HashMap<String, Vec<String>>,
}

/// Reads and merges labels files: one label per line; blank lines and `#` comments are
/// skipped. A line may map its label to a nested keyword: `bird => Wildlife > Bird`. Only the
/// label goes to the model. Labels the model would see as the same ("Makeup", "makeup") are
/// kept once, in the spelling and with the mapping first seen.
pub fn read_labels(paths: &[PathBuf]) -> Result<LabelSet> {
    let mut set = LabelSet::default();
    let mut seen: Vec<String> = Vec::new();
    for path in paths {
        let text =
            fs::read_to_string(path).with_context(|| format!("cannot read labels file {}", path.display()))?;
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        for (index, line) in text.lines().map(str::trim).enumerate() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let at = || format!("{}:{}", path.display(), index + 1);
            let (label, keyword) = parse_label_line(line).with_context(at)?;
            let key = prompt(label);
            if let Some(i) = seen.iter().position(|k| *k == key) {
                let first = &set.labels[i];
                if keyword.is_some() && set.keywords.get(first) != keyword.as_ref() {
                    eprintln!(
                        "warning: {}: \"{label}\" is already listed as \"{first}\" ({}); ignoring this line",
                        at(),
                        set.keywords.get(first).map_or("no keyword mapping".to_string(), |p| p.join(" > "))
                    );
                }
                continue;
            }
            seen.push(key);
            if let Some(keyword) = keyword {
                set.keywords.insert(label.to_string(), keyword);
            }
            set.labels.push(label.to_string());
        }
    }
    ensure!(!set.labels.is_empty(), "no labels in {}", paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", "));
    Ok(set)
}

/// `"bird => Wildlife > Bird"` -> `("bird", Some(["Wildlife", "Bird"]))`; `"bird"` -> `("bird", None)`.
fn parse_label_line(line: &str) -> Result<(&str, Option<Vec<String>>)> {
    let Some((label, keyword)) = line.split_once("=>") else {
        return Ok((line, None));
    };
    let label = label.trim();
    ensure!(!label.is_empty(), "no label before \"=>\" in \"{line}\"");
    let parts: Vec<String> = keyword.split('>').map(|p| p.trim().to_string()).collect();
    ensure!(parts.iter().all(|p| !p.is_empty()), "empty keyword name after \"=>\" in \"{line}\" (expected e.g. \"Wildlife > Bird\")");
    Ok((label, Some(parts)))
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

fn embed_labels(env: &Environment, files: &ModelFiles, labels: &[String], use_gpu: bool) -> Result<Array2<f32>> {
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

    let (mut text_model, _) = open_session(env, &files.text, use_gpu)?;
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

fn open_session(env: &Environment, model: &Path, use_gpu: bool) -> Result<(Session, Backend)> {
    ensure!(model.is_file(), "model file {} not found (run `make fetch` and `make deploy`)", model.display());
    if use_gpu {
        match build_session(env, model, true) {
            Ok(session) => return Ok((session, Backend::DirectML)),
            Err(e) => eprintln!("warning: DirectML unavailable, using the CPU: {e:#}"),
        }
    }
    let session = build_session(env, model, false).with_context(|| format!("cannot load {}", model.display()))?;
    Ok((session, Backend::Cpu))
}

fn build_session(env: &Environment, model: &Path, gpu: bool) -> Result<Session> {
    let mut builder = Session::builder(env).map_err(ort_error)?;
    if gpu {
        // DirectML doesn't support memory patterns or parallel execution.
        builder = builder
            .with_memory_pattern(false)
            .map_err(ort_error)?
            .with_parallel_execution(false)
            .map_err(ort_error)?
            .with_execution_providers([ep::DirectML::default()
                .with_performance_preference(ep::directml::PerformancePreference::HighPerformance)
                .build()
                .error_on_failure()])
            .map_err(ort_error)?;
    }
    builder.commit_from_file(model).map_err(ort_error)
}

/// The name of the GPU Windows ranks first for high performance: the one DirectML picks with
/// `PerformancePreference::HighPerformance` (it sorts adapters the same way).
#[cfg(windows)]
fn high_performance_gpu_name() -> Option<String> {
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory6, DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE,
    };
    // SAFETY: plain DXGI calls; the COM wrappers release the objects when dropped.
    let desc = unsafe {
        let factory: IDXGIFactory6 = CreateDXGIFactory1().ok()?;
        let adapter: IDXGIAdapter1 = factory.EnumAdapterByGpuPreference(0, DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE).ok()?;
        adapter.GetDesc1().ok()?
    };
    let len = desc.Description.iter().position(|&c| c == 0).unwrap_or(desc.Description.len());
    let name = String::from_utf16_lossy(&desc.Description[..len]).trim().to_string();
    (!name.is_empty()).then_some(name)
}

#[cfg(not(windows))]
fn high_performance_gpu_name() -> Option<String> {
    None
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
    fn rank_orders_by_score_and_shares_sum_to_one() {
        let labels: Vec<String> = ["a", "b", "c"].map(String::from).to_vec();
        let all = rank(&labels, &[-5.0, -2.0, -8.0], 3);
        assert_eq!(all.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(), ["b", "a", "c"]);
        assert!((all.iter().map(|s| s.share).sum::<f32>() - 1.0).abs() < 1e-6);
        assert!((all[0].score - sigmoid(-2.0)).abs() < 1e-6);
        assert_eq!(rank(&labels, &[-5.0, -2.0, -8.0], 1).len(), 1);
    }

    #[test]
    fn match_levels_follow_best_score() {
        assert_eq!(MatchLevel::from_best_score(0.12), MatchLevel::Strong);
        assert_eq!(MatchLevel::from_best_score(0.03), MatchLevel::Good);
        assert_eq!(MatchLevel::from_best_score(0.004), MatchLevel::Weak);
    }

    fn scores(values: &[f32]) -> Vec<LabelScore> {
        values.iter().enumerate().map(|(i, &score)| LabelScore { label: format!("l{i}"), score, share: 0.0 }).collect()
    }

    #[test]
    fn suggested_keeps_leading_labels_at_or_above_min() {
        let ranked = scores(&[0.2, 0.01, 0.009, 0.5]);
        // Stops at the first label below the cutoff; the score exactly at it counts.
        assert_eq!(suggested(&ranked, 0.01, 5), &ranked[..2]);
        assert_eq!(suggested(&ranked, 0.3, 5), &[] as &[LabelScore]);
        assert_eq!(suggested(&[], 0.01, 5), &[] as &[LabelScore]);
    }

    #[test]
    fn suggested_is_capped_at_max() {
        let ranked = scores(&[0.9, 0.8, 0.7, 0.6, 0.5, 0.4, 0.3]);
        assert_eq!(suggested(&ranked, SUGGEST_MIN, SUGGEST_MAX).len(), SUGGEST_MAX);
        assert_eq!(suggested(&ranked, SUGGEST_MIN, 2), &ranked[..2]);
    }

    #[test]
    fn label_files_are_merged_without_duplicates() {
        let dir = std::env::temp_dir().join(format!("klassifier-labels-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let set = dir.join("set.txt");
        let hints = dir.join("hints.txt");
        fs::write(&set, "# comment\ndog\n\nmakeup\ndog\n").unwrap();
        fs::write(&hints, "\u{feff}Makeup\nhair styling\r\n").unwrap();
        assert_eq!(read_labels(&[set.clone(), hints.clone()]).unwrap().labels, ["dog", "makeup", "hair styling"]);
        fs::write(&set, "# nothing\n").unwrap();
        assert!(read_labels(&[set]).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    /// A scratch directory for one test; removed by the caller.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("klassifier-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn keyword_mappings_do_not_change_what_the_model_sees() {
        let dir = scratch("mapping-test");
        let plain = dir.join("plain.txt");
        let mapped = dir.join("mapped.txt");
        fs::write(&plain, "# Birds\nbird\nbird in flight\negret\nsunset\n").unwrap();
        fs::write(&mapped, "# Birds\nbird => Wildlife > Bird\nbird in flight=>Wildlife>Bird\n  egret   =>  Wildlife > Bird > Egret \nsunset\n").unwrap();
        let plain = read_labels(&[plain]).unwrap();
        let mapped = read_labels(&[mapped]).unwrap();

        // Same labels, so the same prompts, scores and embedding cache.
        assert_eq!(mapped.labels, plain.labels);
        assert_eq!(mapped.labels.iter().map(|l| prompt(l)).collect::<Vec<_>>(), plain.labels.iter().map(|l| prompt(l)).collect::<Vec<_>>());
        fs::write(dir.join("text_model.onnx"), "t").unwrap();
        fs::write(dir.join("tokenizer.json"), "tok").unwrap();
        let files = ModelFiles::in_dir(&dir);
        assert_eq!(cache_key(&files, &mapped.labels).unwrap(), cache_key(&files, &plain.labels).unwrap());

        assert!(plain.keywords.is_empty());
        assert_eq!(mapped.keywords["bird"], ["Wildlife", "Bird"]);
        assert_eq!(mapped.keywords["bird in flight"], ["Wildlife", "Bird"]);
        assert_eq!(mapped.keywords["egret"], ["Wildlife", "Bird", "Egret"]);
        assert!(!mapped.keywords.contains_key("sunset"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn bad_keyword_mappings_are_errors_with_the_line() {
        for (line, wanted) in [("=> Wildlife", "no label"), ("bird =>", "empty keyword"), ("bird => Wildlife >> Bird", "empty keyword"), ("bird => > Bird", "empty keyword")] {
            let dir = scratch("bad-mapping-test");
            let file = dir.join("set.txt");
            fs::write(&file, format!("dog\n{line}\n")).unwrap();
            let message = format!("{:#}", read_labels(&[file]).unwrap_err());
            assert!(message.contains("set.txt:2") && message.contains(wanted), "{line}: {message}");
            fs::remove_dir_all(&dir).unwrap();
        }
    }

    #[test]
    fn a_duplicate_label_keeps_the_first_mapping() {
        let dir = scratch("duplicate-mapping-test");
        let file = dir.join("set.txt");
        fs::write(&file, "Bird => Wildlife > Bird\nbird => Animals\nsunset\nSunset => Sky\n").unwrap();
        let set = read_labels(&[file]).unwrap();
        assert_eq!(set.labels, ["Bird", "sunset"]);
        assert_eq!(set.keywords["Bird"], ["Wildlife", "Bird"]);
        assert!(!set.keywords.contains_key("sunset"));
        fs::remove_dir_all(&dir).unwrap();
    }

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
        let dir = std::env::temp_dir().join(format!("klassifier-cache-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("c.bin");
        let e = Array2::from_shape_vec((2, 3), vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]).unwrap();
        write_cache(&path, &e).unwrap();
        assert_eq!(read_cache(&path, 2), Some(e));
        assert_eq!(read_cache(&path, 3), None);
        fs::remove_dir_all(&dir).unwrap();
    }
}
