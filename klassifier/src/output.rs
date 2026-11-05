//! Tab-separated output read by the Lightroom plugin.
//!
//! ```text
//! #klassifier<TAB><version>[<TAB>backend=<DirectML|CPU>[<TAB>gpu=<name>]]
//! #model<TAB>name=<model><TAB>load_ms=<n><TAB>labels=<n><TAB>label_cache=<hit|miss>
//! OK<TAB><path><TAB><source><TAB><width><TAB><height><TAB><rotation><TAB><ms><TAB><flags>[<TAB><labels><TAB><shares><TAB><match><TAB><suggested><TAB><keywords>]
//! ERR<TAB><path><TAB><message>
//! #summary<TAB>ok=<n><TAB>err=<n><TAB>ms=<n>
//! ```
//!
//! The last five OK fields, the backend and `#model` are only present when classifying (not
//! with `--no-classify`). `gpu` is the graphics card's name on DirectML, if Windows gives one.
//! - `<labels>`: `label=score;label=score;...`, best first, 0.0000-1.0000. The model's own
//!   score for each label on its own; usually low (a clear match is often 0.05-0.3 with
//!   SigLIP, 0.001-0.05 with SigLIP 2). Use this for cutoffs: it means the same thing in every
//!   photo (for one model).
//! - `<shares>`: the same labels in the same order, `label=share;...`: each label's share of
//!   all labels for this photo (they sum to 1 over all labels). Easier to read, but relative.
//! - `<match>`: `strong`, `good` or `weak`, from the best label's score.
//! - `<suggested>`: `label;label;...`, the labels from `<labels>` suggested as keywords (raw
//!   score at or above `--min-score`, at most `SUGGEST_MAX`), best first. Empty if none.
//! - `<keywords>`: `label=Parent>Child;...`, the Lightroom keyword for each label in `<labels>`
//!   that its labels file maps (`label => Parent > Child`). Labels not listed here are
//!   top-level keywords named as the label. Empty if none is mapped.
//!
//! Lines starting with `#` are informational. Readers must ignore fields beyond the ones
//! they know, so later versions can append fields.

use std::collections::HashMap;

use image::metadata::Orientation;

use crate::preview::{long_edge, Loaded};
use crate::siglip::{self, LabelScore, ModelSpec};

pub fn header(backend: Option<&str>, gpu: Option<&str>) -> String {
    let mut line = format!("#klassifier\t{}", env!("CARGO_PKG_VERSION"));
    if let Some(backend) = backend {
        line.push_str(&format!("\tbackend={backend}"));
        if let Some(gpu) = gpu {
            line.push_str(&format!("\tgpu={}", clean(gpu)));
        }
    }
    line
}

pub fn model_line(model: &ModelSpec, load_ms: u128, labels: usize, from_cache: bool) -> String {
    let cache = if from_cache { "hit" } else { "miss" };
    format!("#model\tname={}\tload_ms={load_ms}\tlabels={labels}\tlabel_cache={cache}", model.name)
}

/// `model` sets the `small` flag (previews under its input size) and the match level;
/// `min_score` is the raw-score cutoff for `<suggested>`; `keywords` the labels' keyword
/// mappings (`LabelSet::keywords`).
pub fn ok_row(
    path: &str,
    loaded: &Loaded,
    model: &ModelSpec,
    ms: u128,
    labels: Option<&[LabelScore]>,
    min_score: f32,
    keywords: &HashMap<String, Vec<String>>,
) -> String {
    let image = &loaded.image;
    let flags = if long_edge(image) < model.image_size { "small" } else { "" };
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
        let mapped: Vec<String> = labels
            .iter()
            .filter_map(|l| {
                let parts = keywords.get(&l.label)?;
                let path: Vec<String> = parts.iter().map(|p| label_name(p).replace('>', " ")).collect();
                Some(format!("{}={}", label_name(&l.label), path.join(">")))
            })
            .collect();
        row.push_str(&format!(
            "\t{}\t{}\t{}\t{}\t{}",
            labels_field(labels, |l| l.score),
            labels_field(labels, |l| l.share),
            model.match_level(best).as_str(),
            suggested.join(";"),
            mapped.join(";")
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
    fn header_names_the_gpu() {
        let version = env!("CARGO_PKG_VERSION");
        assert_eq!(header(None, None), format!("#klassifier\t{version}"));
        assert_eq!(header(Some("CPU"), None), format!("#klassifier\t{version}\tbackend=CPU"));
        assert_eq!(
            header(Some("DirectML"), Some("NVIDIA GeForce RTX 4090")),
            format!("#klassifier\t{version}\tbackend=DirectML\tgpu=NVIDIA GeForce RTX 4090")
        );
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

    fn light() -> &'static ModelSpec {
        &siglip::MODELS[0]
    }

    #[test]
    fn small_flag_follows_the_models_input_size() {
        // A 300 px long edge: above the light model's 224, below SigLIP 2's 384.
        let image = Loaded { image: image::DynamicImage::new_rgb8(300, 200), ..loaded() };
        let flags = |m| ok_row("x.jpg", &image, m, 7, None, 0.01, &HashMap::new()).split('\t').nth(7).unwrap().to_string();
        assert_eq!(flags(light()), "");
        assert_eq!(flags(siglip::model("siglip2-so400m-fp16").unwrap()), "small");
    }

    #[test]
    fn ok_row_lists_suggested_labels() {
        let labels = [
            LabelScore { label: "dog".into(), score: 0.2, share: 0.6 },
            LabelScore { label: "a;b".into(), score: 0.05, share: 0.3 },
            LabelScore { label: "cat".into(), score: 0.004, share: 0.1 },
        ];
        let row = ok_row("x.jpg", &loaded(), light(), 7, Some(&labels), 0.01, &HashMap::new());
        let fields: Vec<&str> = row.split('\t').collect();
        assert_eq!(fields.len(), 13);
        assert_eq!(fields[10], "strong");
        assert_eq!(fields[11], "dog;a b");
        assert_eq!(fields[12], "");
    }

    #[test]
    fn ok_row_suggests_nothing_below_cutoff() {
        let labels = [LabelScore { label: "dog".into(), score: 0.004, share: 1.0 }];
        let row = ok_row("x.jpg", &loaded(), light(), 7, Some(&labels), 0.01, &HashMap::new());
        assert!(row.ends_with("\tweak\t\t"), "{row}");
        assert_eq!(row.split('\t').count(), 13);
    }

    #[test]
    fn ok_row_without_labels_has_eight_fields() {
        assert_eq!(ok_row("x.jpg", &loaded(), light(), 7, None, 0.01, &HashMap::new()).split('\t').count(), 8);
    }

    #[test]
    fn ok_row_lists_keyword_mappings_of_reported_labels() {
        let labels = [
            LabelScore { label: "egret".into(), score: 0.2, share: 0.6 },
            LabelScore { label: "sunset".into(), score: 0.05, share: 0.3 },
        ];
        let keywords = HashMap::from([
            ("egret".to_string(), vec!["Wildlife".to_string(), "Bird".to_string(), "Egret".to_string()]),
            ("odd".to_string(), vec!["A>B;C=D".to_string()]),
            ("owl".to_string(), vec!["Wildlife".to_string()]), // not reported: left out
        ]);
        let row = ok_row("x.jpg", &loaded(), light(), 7, Some(&labels), 0.01, &keywords);
        assert_eq!(row.split('\t').nth(12), Some("egret=Wildlife>Bird>Egret"));

        let odd = [LabelScore { label: "odd".into(), score: 0.2, share: 1.0 }];
        let row = ok_row("x.jpg", &loaded(), light(), 7, Some(&odd), 0.01, &keywords);
        assert_eq!(row.split('\t').nth(12), Some("odd=A B C D"));
    }

    #[test]
    fn every_exif_orientation_has_a_label() {
        for exif in 1..=8 {
            let o = Orientation::from_exif(exif).unwrap();
            assert!(!rotation_label(o).is_empty());
        }
    }
}
