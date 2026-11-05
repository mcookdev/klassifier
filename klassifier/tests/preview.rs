//! Loader tests. The sample-photo tests read `test-data/manifest.tsv` at the repo root (not
//! committed; see README) and are skipped when it is missing. The edge-case tests always run.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use klassifier::preview::{load_image, Source};

fn test_data() -> Option<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../test-data");
    dir.join("manifest.tsv").is_file().then_some(dir)
}

/// A fresh scratch directory for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("klassifier-test-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn error_message(path: &Path) -> String {
    match load_image(path) {
        Ok(_) => panic!("{} loaded but should have failed", path.display()),
        Err(e) => format!("{e:#}"),
    }
}

#[test]
fn sample_photos_load_upright_at_expected_size() {
    let Some(dir) = test_data() else {
        eprintln!("skipping: test-data/manifest.tsv not found");
        return;
    };
    let manifest = fs::read_to_string(dir.join("manifest.tsv")).unwrap();
    let mut failures = Vec::new();
    let mut checked = 0;
    for line in manifest.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')) {
        let fields: Vec<&str> = line.split('\t').collect();
        let [file, source, min_edge, shape] = fields[..] else {
            panic!("bad manifest line: {line:?}");
        };
        let min_edge: u32 = min_edge.parse().unwrap();
        checked += 1;
        match load_image(&dir.join(file)) {
            Err(e) => failures.push(format!("{file}: {e:#}")),
            Ok(loaded) => {
                let (w, h) = (loaded.image.width(), loaded.image.height());
                let got_shape = if h > w { "portrait" } else { "landscape" };
                if loaded.source.as_str() != source {
                    failures.push(format!("{file}: source {} != {source}", loaded.source.as_str()));
                }
                if w.max(h) < min_edge {
                    failures.push(format!("{file}: {w}x{h} smaller than {min_edge}"));
                }
                if got_shape != shape {
                    failures.push(format!("{file}: {w}x{h} is {got_shape}, expected {shape}"));
                }
            }
        }
    }
    assert!(checked > 0, "manifest has no entries");
    assert!(failures.is_empty(), "failures:\n{}", failures.join("\n"));
}

#[test]
fn truncated_raw_fails_cleanly() {
    let Some(dir) = test_data() else {
        eprintln!("skipping: test-data/manifest.tsv not found");
        return;
    };
    let original = fs::read(dir.join("DSC01674-3898.ARW")).unwrap();
    let tmp = scratch("truncated");
    let truncated = tmp.join("truncated.ARW");
    fs::write(&truncated, &original[..64 * 1024]).unwrap();
    error_message(&truncated);
}

#[test]
fn empty_raw_fails_cleanly() {
    let tmp = scratch("empty");
    let empty = tmp.join("empty.ARW");
    fs::write(&empty, b"").unwrap();
    error_message(&empty);
}

#[test]
fn garbage_jpeg_fails_cleanly() {
    let tmp = scratch("garbage");
    let garbage = tmp.join("garbage.jpg");
    fs::write(&garbage, b"this is not a jpeg").unwrap();
    error_message(&garbage);
}

#[test]
fn missing_file_fails() {
    let msg = error_message(Path::new("/nonexistent/dir/IMG_0001.CR2"));
    assert!(msg.contains("cannot open file"), "{msg}");
}

#[test]
fn unsupported_formats_are_reported_as_such() {
    for (name, expected) in [
        ("photo.HEIC", "unsupported: .heic"),
        ("layers.psd", "unsupported: .psd"),
        ("noextension", "unsupported: no file extension"),
    ] {
        assert_eq!(error_message(Path::new(name)), expected);
    }
}

#[test]
fn png_is_read_as_a_file_source() {
    let tmp = scratch("png");
    let png = tmp.join("tiny.png");
    image::RgbImage::new(300, 200).save(&png).unwrap();
    let loaded = load_image(&png).unwrap();
    assert_eq!(loaded.source, Source::File);
    assert_eq!((loaded.image.width(), loaded.image.height()), (300, 200));
}

/// End-to-end check of the output format the Lua plugin parses, including a list file
/// with a BOM and CRLF line endings.
#[test]
fn cli_reports_each_listed_path() {
    let tmp = scratch("cli");
    let small = tmp.join("small.png");
    image::RgbImage::new(100, 50).save(&small).unwrap();
    let missing = tmp.join("missing.ARW");
    let list = tmp.join("list.txt");
    fs::write(&list, format!("\u{feff}{}\r\n{}\r\n\r\n", small.display(), missing.display())).unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_klassifier")).arg("--no-classify").arg("--list").arg(&list).output().unwrap();
    assert!(out.status.success(), "exit {:?}: {}", out.status, String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();

    assert!(lines[0].starts_with("#klassifier\t"), "{stdout}");
    assert_eq!(lines[1], format!("OK\t{}\tfile\t100\t50\t0\t{}\tsmall", small.display(), lines[1].split('\t').nth(6).unwrap()));
    assert!(lines[2].starts_with(&format!("ERR\t{}\tcannot open file", missing.display())), "{stdout}");
    assert!(lines[3].starts_with("#summary\tok=1\terr=1\t"), "{stdout}");
    assert_eq!(lines.len(), 4, "{stdout}");
}

#[test]
fn cli_without_paths_is_a_usage_error() {
    let out = Command::new(env!("CARGO_BIN_EXE_klassifier")).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}
