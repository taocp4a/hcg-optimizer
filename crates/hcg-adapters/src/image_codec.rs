//! [`ImageCodec`] implemented with the pure-Rust `image` crate (JPEG, PNG, BMP). Builds for
//! native and wasm32.

use std::collections::HashMap;
use std::io::Cursor;

use hcg_app::policy::fit_within;
use hcg_app::ports::{ImageCodec, ImageFormat, ImageProbe, OutputFormat};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
use image::imageops::FilterType;
use image::{
    DynamicImage, ExtendedColorType, GenericImageView, ImageDecoder, ImageEncoder, ImageReader,
    Limits,
};

/// Decoded, downscaled pixels plus the EXIF orientation of the source (1 = none).
pub struct CodecImage {
    pixels: DynamicImage,
    orientation: u8,
}

#[derive(Debug, thiserror::Error)]
pub enum ImageCodecError {
    #[error(transparent)]
    Image(#[from] image::ImageError),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("format not supported by this codec")]
    Unsupported,
}

/// The default codec. `max_alloc` caps decoder memory (bytes) to protect wasm32 memory.
#[derive(Debug, Clone, Copy)]
pub struct PureRustCodec {
    pub max_alloc: u64,
}

impl Default for PureRustCodec {
    /// 1 GiB: a 100-megapixel RGBA image is 400 MB.
    fn default() -> Self {
        Self { max_alloc: 1 << 30 }
    }
}

fn detect(bytes: &[u8]) -> ImageFormat {
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return ImageFormat::Gif;
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return ImageFormat::WebP;
    }
    match image::guess_format(bytes) {
        Ok(image::ImageFormat::Jpeg) => ImageFormat::Jpeg,
        Ok(image::ImageFormat::Png) => ImageFormat::Png,
        Ok(image::ImageFormat::Bmp) => ImageFormat::Bmp,
        _ => ImageFormat::Other,
    }
}

fn crate_format(format: ImageFormat) -> Option<image::ImageFormat> {
    match format {
        ImageFormat::Jpeg => Some(image::ImageFormat::Jpeg),
        ImageFormat::Png => Some(image::ImageFormat::Png),
        ImageFormat::Bmp => Some(image::ImageFormat::Bmp),
        _ => None,
    }
}

/// Minimal TIFF block holding one EXIF Orientation (0x0112) entry, little-endian. The JPEG
/// encoder adds the `Exif\0\0` header itself.
fn orientation_exif(orientation: u8) -> Vec<u8> {
    let mut tiff = Vec::with_capacity(26);
    tiff.extend_from_slice(b"II*\0");
    tiff.extend_from_slice(&8u32.to_le_bytes()); // IFD0 offset
    tiff.extend_from_slice(&1u16.to_le_bytes()); // one entry
    tiff.extend_from_slice(&0x0112u16.to_le_bytes()); // Orientation
    tiff.extend_from_slice(&3u16.to_le_bytes()); // SHORT
    tiff.extend_from_slice(&1u32.to_le_bytes()); // count
    tiff.extend_from_slice(&u16::from(orientation).to_le_bytes());
    tiff.extend_from_slice(&[0, 0]); // value padding
    tiff.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
    tiff
}

impl ImageCodec for PureRustCodec {
    type Image = CodecImage;
    type Error = ImageCodecError;

    fn probe(&self, bytes: &[u8]) -> Result<ImageProbe, ImageCodecError> {
        let format = detect(bytes);
        let dimensions = match crate_format(format) {
            Some(f) => Some(ImageReader::with_format(Cursor::new(bytes), f).into_dimensions()?),
            None => None,
        };
        Ok(ImageProbe { format, dimensions })
    }

    fn decode_fit(
        &self,
        bytes: &[u8],
        max_w: u32,
        max_h: u32,
    ) -> Result<CodecImage, ImageCodecError> {
        let format = crate_format(detect(bytes)).ok_or(ImageCodecError::Unsupported)?;
        let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
        let mut limits = Limits::default();
        limits.max_alloc = Some(self.max_alloc);
        reader.limits(limits);
        let mut decoder = reader.into_decoder()?;
        let orientation = decoder.orientation().map_or(1, |o| o.to_exif());
        let pixels = DynamicImage::from_decoder(decoder)?;
        let (w, h) = pixels.dimensions();
        let (tw, th) = fit_within(w, h, max_w, max_h);
        let pixels = if (tw, th) == (w, h) {
            pixels
        } else {
            pixels.resize_exact(tw, th, FilterType::Lanczos3)
        };
        Ok(CodecImage {
            pixels,
            orientation,
        })
    }

    fn dimensions(&self, image: &CodecImage) -> (u32, u32) {
        image.pixels.dimensions()
    }

    fn uses_alpha(&self, image: &CodecImage) -> bool {
        match &image.pixels {
            DynamicImage::ImageLumaA8(i) => i.pixels().any(|p| p.0[1] < u8::MAX),
            DynamicImage::ImageRgba8(i) => i.pixels().any(|p| p.0[3] < u8::MAX),
            DynamicImage::ImageLumaA16(i) => i.pixels().any(|p| p.0[1] < u16::MAX),
            DynamicImage::ImageRgba16(i) => i.pixels().any(|p| p.0[3] < u16::MAX),
            DynamicImage::ImageRgba32F(i) => i.pixels().any(|p| p.0[3] < 1.0),
            other if other.color().has_alpha() => {
                other.to_rgba8().pixels().any(|p| p.0[3] < u8::MAX)
            }
            _ => false,
        }
    }

    fn dominant_border_color(&self, image: &CodecImage) -> Option<([u8; 3], f32)> {
        let img = &image.pixels;
        let (w, h) = img.dimensions();
        if w == 0 || h == 0 {
            return None;
        }
        let mut counts: HashMap<[u8; 3], u32> = HashMap::new();
        let mut total = 0u32;
        let mut add = |x: u32, y: u32| {
            let p = img.get_pixel(x, y).0;
            *counts.entry([p[0], p[1], p[2]]).or_default() += 1;
            total += 1;
        };
        for x in 0..w {
            add(x, 0);
            if h > 1 {
                add(x, h - 1);
            }
        }
        for y in 1..h.saturating_sub(1) {
            add(0, y);
            if w > 1 {
                add(w - 1, y);
            }
        }
        // Ties broken by colour value so the result is deterministic.
        let (rgb, n) = counts
            .into_iter()
            .max_by_key(|&(rgb, n)| (n, std::cmp::Reverse(rgb)))?;
        Some((rgb, n as f32 / total as f32))
    }

    fn encode(&self, image: &CodecImage, format: OutputFormat) -> Result<Vec<u8>, ImageCodecError> {
        let mut out = Vec::new();
        let exif = (image.orientation != 1).then(|| orientation_exif(image.orientation));
        match format {
            OutputFormat::Jpeg { quality } => {
                let rgb = image.pixels.to_rgb8();
                let mut encoder = JpegEncoder::new_with_quality(&mut out, quality);
                if let Some(exif) = exif {
                    let _ = encoder.set_exif_metadata(exif);
                }
                encoder.write_image(
                    rgb.as_raw(),
                    rgb.width(),
                    rgb.height(),
                    ExtendedColorType::Rgb8,
                )?;
            }
            OutputFormat::Png => {
                let mut encoder = PngEncoder::new_with_quality(
                    &mut out,
                    CompressionType::Default,
                    PngFilter::Adaptive,
                );
                if let Some(exif) = exif {
                    let _ = encoder.set_exif_metadata(exif);
                }
                if image.pixels.color().has_alpha() {
                    let rgba = image.pixels.to_rgba8();
                    encoder.write_image(
                        rgba.as_raw(),
                        rgba.width(),
                        rgba.height(),
                        ExtendedColorType::Rgba8,
                    )?;
                } else {
                    let rgb = image.pixels.to_rgb8();
                    encoder.write_image(
                        rgb.as_raw(),
                        rgb.width(),
                        rgb.height(),
                        ExtendedColorType::Rgb8,
                    )?;
                }
            }
        }
        Ok(out)
    }
}
