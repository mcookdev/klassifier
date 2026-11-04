//! Tab-separated output read by the Lightroom plugin.
//!
//! ```text
//! #hello-app<TAB><version>
//! OK<TAB><path><TAB><source><TAB><width><TAB><height><TAB><rotation><TAB><ms><TAB><flags>
//! ERR<TAB><path><TAB><message>
//! #summary<TAB>ok=<n><TAB>err=<n><TAB>ms=<n>
//! ```
//!
//! Lines starting with `#` are informational. Readers must ignore fields beyond the ones
//! they know, so later versions can append fields.

use image::metadata::Orientation;

use crate::preview::{long_edge, Loaded};

/// Images whose long edge is below this are flagged `small`; it is the classifier's input size.
pub const SMALL_EDGE: u32 = 224;

pub fn header() -> String {
    format!("#hello-app\t{}", env!("CARGO_PKG_VERSION"))
}

pub fn ok_row(path: &str, loaded: &Loaded, ms: u128) -> String {
    let image = &loaded.image;
    let flags = if long_edge(image) < SMALL_EDGE { "small" } else { "" };
    format!(
        "OK\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        clean(path),
        loaded.source.as_str(),
        image.width(),
        image.height(),
        rotation_label(loaded.orientation),
        ms,
        flags
    )
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
    fn every_exif_orientation_has_a_label() {
        for exif in 1..=8 {
            let o = Orientation::from_exif(exif).unwrap();
            assert!(!rotation_label(o).is_empty());
        }
    }
}
