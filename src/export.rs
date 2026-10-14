//! Image export in PNG, JPEG and TIFF.
use crate::tex::Tex;
use crate::{jpeg, png, tiff};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg,
    Tiff,
}

impl Format {
    pub fn ext(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpeg => "jpg",
            Format::Tiff => "tif",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Format::Png => "PNG",
            Format::Jpeg => "JPEG",
            Format::Tiff => "TIFF",
        }
    }
    pub fn parse(s: &str) -> Option<Format> {
        match s.to_ascii_lowercase().as_str() {
            "png" => Some(Format::Png),
            "jpg" | "jpeg" => Some(Format::Jpeg),
            "tif" | "tiff" => Some(Format::Tiff),
            _ => None,
        }
    }
}

/// `depth` is 8 or 16 (ignored for JPEG); `quality` 1..=100 (JPEG only).
pub fn encode(fmt: Format, t: &Tex, depth: u8, quality: u8) -> Vec<u8> {
    match fmt {
        Format::Png => png::encode(t.w, t.h, &t.px, depth),
        Format::Jpeg => jpeg::encode(t, quality),
        Format::Tiff => tiff::encode(t, depth),
    }
}
