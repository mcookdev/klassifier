//! Tab-separated output read by the Lightroom plugin.
//!
//! ```text
//! #hello-app<TAB><version>[<TAB>backend=<DirectML|CPU>]
//! #model<TAB>load_ms=<n><TAB>labels=<n><TAB>label_cache=<hit|miss>
//! OK<TAB><path><TAB><source><TAB><width><TAB><height><TAB><rotation><TAB><ms><TAB><flags>[<TAB><labels><TAB><shares><TAB><match><TAB><suggested>]
//! ERR<TAB><path><TAB><message>
//! #summary<TAB>ok=<n><TAB>err=<n><TAB>ms=<n>
//! ```
//!
//! The last four OK fields, the backend and `#model` are only present when classifying (not
//! with `--no-classify`):
//! - `<labels>`: `label=score;label=score;...`, best first, 0.0000-1.0000. SigLIP's own score
//!   for each label on its own; usually low (a clear match is often 0.05-0.3). Use this for
//!   cutoffs: it means the same thing in every photo.
//! - `<shares>`: the same labels in the same order, `label=share;...`: each label's share of
//!   all labels for this photo (they sum to 1 over all labels). Easier to read, but relative.
//! - `<match>`: `strong`, `good` or `weak`, from the best label's score.
//! - `<suggested>`: `label;label;...`, the labels from `<labels>` suggested as keywords (raw
//!   score at or above `--min-score`, at most `SUGGEST_MAX`), best first. Empty if none.
//!
//! Lines starting with `#` are informational. Readers must ignore fields beyond the ones
//! they know, so later versions can append fields.

use image::metadata::Orientation;

use crate::preview::{long_edge, Loaded};
use crate::siglip::{self, LabelScore, MatchLevel};

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

/// `min_score` is the raw-score cutoff for `<suggested>`.
pub fn ok_row(path: &str, loaded: &Loaded, ms: u128, labels: Option<&[LabelScore]>, min_score: f32) -> String {
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
        let best = labels.first().map_or(0.0, |l| l.score);
        let suggested: Vec<String> =
            siglip::suggested(labels, min_score, siglip::SUGGEST_MAX).iter().map(|l| label_name(&l.label)).collect();
        row.push_str(&format!(
            "\t{}\t{}\t{}\t{}",
            labels_field(labels, |l| l.score),
            labels_field(labels, |l| l.share),
            MatchLevel::from_best_score(best).as_str(),
            suggested.join(";")
        ));
    }
    row
}

/// `label=0.9100;label=0.6400`, with the separators removed from labels.
fn labels_field(labels: &[LabelScore], value: impl Fn(&LabelScore) -> f32) -> String {
    labels
        .iter()
        .map(|l| format!("{}={:.4}", label_name(&l.label), value(l)))
        .collect::<Vec<_>>()
        .join(";")
}

/// A label as it appears in the label fields, without the field and list separators.
fn label_name(label: &str) -> String {
    clean(label).replace(['=', ';'], " ")
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
        let labels = [
            LabelScore { label: "dog".into(), score: 0.912, share: 0.7 },
            LabelScore { label: "a=b;c".into(), score: 0.5, share: 0.2 },
        ];
        assert_eq!(labels_field(&labels, |l| l.score), "dog=0.9120;a b c=0.5000");
        assert_eq!(labels_field(&labels, |l| l.share), "dog=0.7000;a b c=0.2000");
    }

    fn loaded() -> Loaded {
        Loaded {
            image: image::DynamicImage::new_rgb8(400, 300),
            source: crate::preview::Source::File,
            orientation: Orientation::NoTransforms,
        }
    }

    #[test]
    fn ok_row_lists_suggested_labels() {
        let labels = [
            LabelScore { label: "dog".into(), score: 0.2, share: 0.6 },
            LabelScore { label: "a;b".into(), score: 0.05, share: 0.3 },
            LabelScore { label: "cat".into(), score: 0.004, share: 0.1 },
        ];
        let row = ok_row("x.jpg", &loaded(), 7, Some(&labels), 0.01);
        let fields: Vec<&str> = row.split('\t').collect();
        assert_eq!(fields.len(), 12);
        assert_eq!(fields[10], "strong");
        assert_eq!(fields[11], "dog;a b");
    }

    #[test]
    fn ok_row_suggests_nothing_below_cutoff() {
        let labels = [LabelScore { label: "dog".into(), score: 0.004, share: 1.0 }];
        let row = ok_row("x.jpg", &loaded(), 7, Some(&labels), 0.01);
        assert!(row.ends_with("\tweak\t"), "{row}");
        assert_eq!(row.split('\t').count(), 12);
    }

    #[test]
    fn ok_row_without_labels_has_eight_fields() {
        assert_eq!(ok_row("x.jpg", &loaded(), 7, None, 0.01).split('\t').count(), 8);
    }

    #[test]
    fn every_exif_orientation_has_a_label() {
        for exif in 1..=8 {
            let o = Orientation::from_exif(exif).unwrap();
            assert!(!rotation_label(o).is_empty());
        }
    }
}
