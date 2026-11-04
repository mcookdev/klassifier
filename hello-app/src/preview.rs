//! Loads a viewable image for a photo file.
//!
//! Ordinary images (JPEG, PNG, TIFF, WebP) are decoded directly. For camera RAW files the
//! largest embedded JPEG is used; raw sensor data is never decoded. The result is rotated
//! upright according to the file's EXIF orientation.

use std::panic::{self, AssertUnwindSafe};
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageReader};
use rawler::decoders::{Decoder, RawDecodeParams};
use rawler::rawsource::RawSource;

const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "jpe", "png", "tif", "tiff", "webp"];
const RAW_EXTENSIONS: &[&str] = &[
    "3fr", "arw", "cr2", "cr3", "crw", "dcr", "dng", "erf", "iiq", "kdc", "mef", "mos", "mrw",
    "nef", "nrw", "orf", "pef", "raf", "rw2", "sr2", "srf", "srw",
];

/// An embedded image this large is used as soon as it is found; smaller ones only if
/// nothing larger turns up.
const GOOD_ENOUGH_EDGE: u32 = 1024;

/// Where the image came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Decoded directly from an ordinary image file.
    File,
    /// A RAW file's embedded preview JPEG.
    Preview,
    /// A RAW file's embedded full-size JPEG.
    Full,
    /// A RAW file's embedded thumbnail (on some formats, e.g. iPhone DNG, this is the large one).
    Thumbnail,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::File => "file",
            Source::Preview => "preview",
            Source::Full => "full",
            Source::Thumbnail => "thumbnail",
        }
    }
}

pub struct Loaded {
    /// Upright image (orientation already applied).
    pub image: DynamicImage,
    pub source: Source,
    /// The orientation that was applied to make the image upright.
    pub orientation: Orientation,
}

/// Loads `path` as an upright image. Unsupported formats return an error whose message
/// starts with `unsupported:`.
pub fn load_image(path: &Path) -> Result<Loaded> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();

    let load: fn(&Path) -> Result<Loaded> = if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        load_image_file
    } else if RAW_EXTENSIONS.contains(&ext.as_str()) {
        load_raw_file
    } else if ext.is_empty() {
        bail!("unsupported: no file extension");
    } else {
        bail!("unsupported: .{ext}");
    };

    // A decoder panicking on a corrupt file must not take the whole batch down.
    panic::catch_unwind(AssertUnwindSafe(|| load(path))).unwrap_or_else(|payload| {
        let msg = payload
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".into());
        Err(anyhow!("decoder crashed: {msg}"))
    })
}

fn load_image_file(path: &Path) -> Result<Loaded> {
    let mut decoder = ImageReader::open(path)
        .context("cannot open file")?
        .with_guessed_format()
        .context("cannot read file")?
        .into_decoder()
        .context("cannot decode image")?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder).context("cannot decode image")?;
    image.apply_orientation(orientation);
    Ok(Loaded { image, source: Source::File, orientation })
}

fn load_raw_file(path: &Path) -> Result<Loaded> {
    let src = RawSource::new(path).context("cannot open file")?;
    let decoder = rawler::get_decoder(&src).context("cannot read RAW file")?;
    let params = RawDecodeParams::default();

    let (mut image, source) = largest_embedded_image(decoder.as_ref(), &src, &params)?;

    // Embedded previews are stored as shot (sensor orientation), so rotate them upright.
    let orientation = decoder
        .raw_metadata(&src, &params)
        .ok()
        .and_then(|m| m.exif.orientation)
        .and_then(|o| u8::try_from(o).ok())
        .and_then(Orientation::from_exif)
        .unwrap_or(Orientation::NoTransforms);
    image.apply_orientation(orientation);
    Ok(Loaded { image, source, orientation })
}

/// Tries the embedded preview, full-size and thumbnail images in that order. Returns the
/// first one that is at least `GOOD_ENOUGH_EDGE` on its long edge, else the largest found.
fn largest_embedded_image(
    decoder: &dyn Decoder,
    src: &RawSource,
    params: &RawDecodeParams,
) -> Result<(DynamicImage, Source)> {
    let candidates: [(Source, fn(&dyn Decoder, &RawSource, &RawDecodeParams) -> _); 3] = [
        (Source::Preview, |d, s, p| d.preview_image(s, p)),
        (Source::Full, |d, s, p| d.full_image(s, p)),
        (Source::Thumbnail, |d, s, p| d.thumbnail_image(s, p)),
    ];

    let mut best: Option<(DynamicImage, Source)> = None;
    let mut last_error = None;
    for (source, get) in candidates {
        match get(decoder, src, params) {
            Ok(Some(image)) => {
                let edge = long_edge(&image);
                if edge >= GOOD_ENOUGH_EDGE {
                    return Ok((image, source));
                }
                if best.as_ref().is_none_or(|(b, _)| edge > long_edge(b)) {
                    best = Some((image, source));
                }
            }
            Ok(None) => {}
            Err(e) => last_error = Some(e),
        }
    }
    match (best, last_error) {
        (Some(found), _) => Ok(found),
        (None, Some(e)) => Err(anyhow!(e).context("cannot read embedded preview")),
        (None, None) => bail!("no embedded preview"),
    }
}

pub fn long_edge(image: &DynamicImage) -> u32 {
    image.width().max(image.height())
}
