//! Tab-separated output read by the Lightroom plugin.
//!
//! ```text
//! #hello-app<TAB><version>[<TAB>backend=<DirectML|CPU>]
//! #model<TAB>load_ms=<n><TAB>labels=<n><TAB>label_cache=<hit|miss>
//! OK<TAB><path><TAB><source><TAB><width><TAB><height><TAB><rotation><TAB><ms><TAB><flags>[<TAB><labels>]
//! ERR<TAB><path><TAB><message>
//! #summary<TAB>ok=<n><TAB>err=<n><TAB>ms=<n>
//! ```
//!
//! `<labels>` is `label=score;label=score;...`, highest score first, scores 0.0000-1.0000.
//! SigLIP scores are independent per label and usually low (a clear match is often 0.05-0.3),
//! so compare them within a photo rather than treating them as probabilities. It and
//! the backend are absent with `--no-classify`. `#model` is only printed when classifying.
//!
//! Lines starting with `#` are informational. Readers must ignore fields beyond the ones
//! they know, so later versions can append fields.

use image::metadata::Orientation;

use crate::preview::{long_edge, Loaded};

/// Images whose long edge is below this are flagged `small`; it is the classifier's input size.
pub const SMALL_EDGE: u32 = 224;

pub fn header(backend: Option<&str>) -> String {
    let mut line = format!("#hello-app\t{}", env!("CARGO_PKG_VERSION"));
    if let Some(backend) = backend {
        line.push_str(&format!("\tbackend={backend}"));
    }
    line
}

pub fn model_line(load_ms: u128, labels: usize, from_cache: bool) -> String {
    let cache = if from_cache { "hit" } else { "miss" };
    format!("#model\tload_ms={load_ms}\tlabels={labels}\tlabel_cache={cache}")
}

pub fn ok_row(path: &str, loaded: &Loaded, ms: u128, labels: Option<&[(String, f32)]>) -> String {
    let image = &loaded.image;
    let flags = if long_edge(image) < SMALL_EDGE { "small" } else { "" };
    let mut row = format!(
        "OK\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        clean(path),
        loaded.source.as_str(),
        image.width(),
        image.height(),
        rotation_label(loaded.orientation),
        ms,
        flags
    );
    if let Some(labels) = labels {
        row.push('\t');
        row.push_str(&labels_field(labels));
    }
    row
}

/// `label=0.9100;label=0.6400`, with the separators removed from labels.
fn labels_field(labels: &[(String, f32)]) -> String {
    labels
        .iter()
        .map(|(label, score)| format!("{}={score:.4}", clean(label).replace(['=', ';'], " ")))
        .collect::<Vec<_>>()
        .join(";")
}

pub fn err_row(path: &str, error: &anyhow::Error) -> String {
    // `{:#}` includes the cause chain, e.g. "cannot open file: No such file or directory".
    format!("ERR\t{}\t{}", clean(path), clean(&format!("{error:#}")))
}

pub fn summary(ok: usize, err: usize, ms: u128) -> String {
    format!("#summary\tok={ok}\terr={err}\tms={ms}")
}

/// Degrees rotated clockwise, with `-flip` when the image was also mirrored.
pub fn rotation_label(orientation: Orientation) -> &'static str {
    match orientation {
        Orientation::NoTransforms => "0",
        Orientation::Rotate90 => "90",
        Orientation::Rotate180 => "180",
        Orientation::Rotate270 => "270",
        Orientation::FlipHorizontal => "0-flip",
        Orientation::FlipVertical => "180-flip",
        Orientation::Rotate90FlipH => "90-flip",
        Orientation::Rotate270FlipH => "270-flip",
    }
}

/// Keeps a value on one line and in one field.
fn clean(s: &str) -> String {
    s.replace(['\t', '\r', '\n'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_removes_separators() {
        assert_eq!(clean("a\tb\r\nc"), "a b  c");
    }

    #[test]
    fn err_row_is_one_line_with_three_fields() {
        let error = anyhow::anyhow!("bad\nthing").context("outer");
        let row = err_row("C:\\x.ARW", &error);
        assert_eq!(row, "ERR\tC:\\x.ARW\touter: bad thing");
    }

    #[test]
    fn labels_field_escapes_separators() {
        assert_eq!(labels_field(&[("dog".into(), 0.912), ("a=b;c".into(), 0.5)]), "dog=0.9120;a b c=0.5000");
    }

    #[test]
    fn every_exif_orientation_has_a_label() {
        for exif in 1..=8 {
            let o = Orientation::from_exif(exif).unwrap();
            assert!(!rotation_label(o).is_empty());
        }
    }
}
