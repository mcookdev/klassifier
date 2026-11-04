//! Reads a preview image for each photo given and reports it in the tab-separated format
//! described in `output.rs`. One photo failing does not stop the others.

use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use hello_app::{output, preview};

const USAGE: &str = "\
Usage: hello-app [--list <file>] [--dump <dir>] [paths...]

  --list <file>  Read photo paths from <file>, one per line (UTF-8)
  --dump <dir>   Save each loaded image, at most 1024 px, as <dir>/<file name>.jpg";

/// Long edge of images written by `--dump`.
const DUMP_EDGE: u32 = 1024;

struct Args {
    paths: Vec<PathBuf>,
    dump: Option<PathBuf>,
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
    let mut paths = Vec::new();
    let mut dump = None;
    let mut it = raw.into_iter();
    while let Some(arg) = it.next() {
        match arg.to_str() {
            Some("-h" | "--help") => return Ok(None),
            Some("--list") => {
                let file = it.next().context("--list needs a file")?;
                let text = fs::read_to_string(&file)
                    .with_context(|| format!("cannot read list file {}", file.to_string_lossy()))?;
                let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
                paths.extend(
                    text.lines()
                        .map(str::trim_end)
                        .filter(|l| !l.is_empty())
                        .map(PathBuf::from),
                );
            }
            Some("--dump") => dump = Some(PathBuf::from(it.next().context("--dump needs a directory")?)),
            Some(s) if s.starts_with("--") => bail!("unknown option {s}"),
            _ => paths.push(PathBuf::from(arg)),
        }
    }
    if paths.is_empty() {
        bail!("no photos given");
    }
    Ok(Some(Args { paths, dump }))
}

fn run(args: &Args) -> Result<()> {
    if let Some(dir) = &args.dump {
        fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    }

    let mut out = io::stdout().lock();
    writeln!(out, "{}", output::header())?;

    let started = Instant::now();
    let (mut ok, mut err) = (0, 0);
    for path in &args.paths {
        let shown = path.to_string_lossy();
        let t = Instant::now();
        match preview::load_image(path) {
            Ok(loaded) => {
                let ms = t.elapsed().as_millis();
                writeln!(out, "{}", output::ok_row(&shown, &loaded, ms))?;
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
