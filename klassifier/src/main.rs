//! Reads a preview image for each photo given, classifies it with SigLIP, and reports both
//! in the tab-separated format described in `output.rs`. One photo failing does not stop
//! the others.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use klassifier::siglip::{self, Classifier, ModelFiles};
use klassifier::{output, preview};

const USAGE: &str = "\
Usage: klassifier [options] [paths...]

  --list <file>    Read photo paths from <file>, one per line (UTF-8)
  --top <n>        Report the <n> best-scoring labels per photo (default 5)
  --min-score <x>  Suggest labels with a raw score of at least <x>, 0 to 1 (default 0.01)
  --labels <file>  Labels to score against; repeat to merge several files
                   (default: label-sets/General.txt next to the exe)
  --cpu            Don't use the GPU
  --no-classify    Only read the previews; needs no model files
  --dump <dir>     Save each loaded image, at most 1024 px, as <dir>/<file name>.jpg";

/// Long edge of images written by `--dump`.
const DUMP_EDGE: u32 = 1024;
const DEFAULT_TOP: usize = 5;

struct Args {
    paths: Vec<PathBuf>,
    dump: Option<PathBuf>,
    top: usize,
    labels: Vec<PathBuf>,
    min_score: f32,
    classify: bool,
    use_gpu: bool,
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args_os().skip(1).collect()) {
        Ok(Some(args)) => args,
        Ok(None) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("error: {e:#}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// Returns `None` when help was requested.
fn parse_args(raw: Vec<OsString>) -> Result<Option<Args>> {
    let mut args = Args {
        paths: Vec::new(),
        dump: None,
        top: DEFAULT_TOP,
        labels: Vec::new(),
        min_score: siglip::SUGGEST_MIN,
        classify: true,
        use_gpu: true,
    };
    let mut it = raw.into_iter();
    while let Some(arg) = it.next() {
        match arg.to_str() {
            Some("-h" | "--help") => return Ok(None),
            Some("--list") => {
                let file = it.next().context("--list needs a file")?;
                let text = fs::read_to_string(&file)
                    .with_context(|| format!("cannot read list file {}", file.to_string_lossy()))?;
                let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
                args.paths.extend(
                    text.lines()
                        .map(str::trim_end)
                        .filter(|l| !l.is_empty())
                        .map(PathBuf::from),
                );
            }
            Some("--dump") => args.dump = Some(PathBuf::from(it.next().context("--dump needs a directory")?)),
            Some("--labels") => args.labels.push(PathBuf::from(it.next().context("--labels needs a file")?)),
            Some("--top") => {
                let n = it.next().context("--top needs a number")?;
                args.top = n
                    .to_str()
                    .and_then(|s| s.parse().ok())
                    .filter(|&n| n > 0)
                    .context("--top needs a number above 0")?;
            }
            Some("--min-score") => {
                let x = it.next().context("--min-score needs a number from 0 to 1")?;
                args.min_score = x
                    .to_str()
                    .and_then(|s| s.parse().ok())
                    .filter(|x: &f32| (0.0..=1.0).contains(x))
                    .context("--min-score needs a number from 0 to 1")?;
            }
            Some("--cpu") => args.use_gpu = false,
            Some("--no-classify") => args.classify = false,
            Some(s) if s.starts_with("--") => bail!("unknown option {s}"),
            _ => args.paths.push(PathBuf::from(arg)),
        }
    }
    if args.paths.is_empty() {
        bail!("no photos given");
    }
    Ok(Some(args))
}

/// Loads ONNX Runtime, the model and the labels from the exe's folder (laid out by `make deploy`).
/// Also returns the labels' keyword mappings.
fn load_classifier(args: &Args) -> Result<(Classifier, HashMap<String, Vec<String>>, u128)> {
    let started = Instant::now();
    let exe = std::env::current_exe().context("cannot find the exe's folder")?;
    let dir = exe.parent().context("cannot find the exe's folder")?;
    let label_files = if args.labels.is_empty() {
        vec![dir.join("label-sets").join("General.txt")]
    } else {
        args.labels.clone()
    };
    let set = siglip::read_labels(&label_files)?;
    siglip::init_runtime(dir)?;
    let files = ModelFiles::in_dir(&dir.join("models").join("siglip"));
    let classifier = Classifier::new(&files, set.labels, &std::env::temp_dir(), args.use_gpu)?;
    Ok((classifier, set.keywords, started.elapsed().as_millis()))
}

fn run(args: &Args) -> Result<()> {
    if let Some(dir) = &args.dump {
        fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    }

    let mut classifier = None;
    let mut keywords = HashMap::new();
    let mut out = io::stdout().lock();
    if args.classify {
        let (c, k, load_ms) = load_classifier(args)?;
        keywords = k;
        writeln!(out, "{}", output::header(Some(c.backend().as_str())))?;
        writeln!(out, "{}", output::model_line(load_ms, c.label_count(), c.labels_from_cache()))?;
        classifier = Some(c);
    } else {
        writeln!(out, "{}", output::header(None))?;
    }

    let started = Instant::now();
    let (mut ok, mut err) = (0, 0);
    for path in &args.paths {
        let shown = path.to_string_lossy();
        let t = Instant::now();
        let result = preview::load_image(path).and_then(|loaded| {
            let labels = match classifier.as_mut() {
                Some(c) => Some(c.classify(&loaded.image, args.top).context("cannot classify")?),
                None => None,
            };
            Ok((loaded, labels))
        });
        match result {
            Ok((loaded, labels)) => {
                let ms = t.elapsed().as_millis();
                writeln!(out, "{}", output::ok_row(&shown, &loaded, ms, labels.as_deref(), args.min_score, &keywords))?;
                ok += 1;
                if let Some(dir) = &args.dump {
                    if let Err(e) = dump(dir, path, &loaded.image) {
                        eprintln!("warning: cannot dump {shown}: {e:#}");
                    }
                }
            }
            Err(e) => {
                writeln!(out, "{}", output::err_row(&shown, &e))?;
                err += 1;
            }
        }
        out.flush()?;
    }
    writeln!(out, "{}", output::summary(ok, err, started.elapsed().as_millis()))?;
    Ok(())
}

fn dump(dir: &Path, path: &Path, image: &image::DynamicImage) -> Result<()> {
    let name = path.file_name().context("path has no file name")?;
    let mut target = dir.join(name);
    target.as_mut_os_string().push(".jpg");
    image.thumbnail(DUMP_EDGE, DUMP_EDGE).to_rgb8().save(&target)?;
    Ok(())
}
