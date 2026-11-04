//! [`ImageCodec`] implemented with the pure-Rust `image` crate. Decodes and encodes JPEG, PNG
//! and BMP; recognises GIF, DDS, TGA, WebP and HEIC and reads the size of GIF, DDS and TGA from
//! their headers (for the report). Builds for native and wasm32.

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

/// Format from the leading signature bytes; TGA (no signature) from the file extension.
fn detect(bytes: &[u8], file_name: &str) -> ImageFormat {
    let at = |range: std::ops::Range<usize>| bytes.get(range).unwrap_or_default();
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return ImageFormat::Gif;
    }
    if bytes.starts_with(b"DDS ") {
        return ImageFormat::Dds;
    }
    if bytes.starts_with(b"RIFF") && at(8..12) == b"WEBP" {
        return ImageFormat::WebP;
    }
    const HEIF_BRANDS: [&[u8]; 9] = [
        b"heic", b"heix", b"hevc", b"hevx", b"heim", b"heis", b"hevm", b"mif1", b"msf1",
    ];
    if at(4..8) == b"ftyp" && HEIF_BRANDS.contains(&at(8..12)) {
        return ImageFormat::Heic;
    }
    match image::guess_format(bytes) {
        Ok(image::ImageFormat::Jpeg) => return ImageFormat::Jpeg,
        Ok(image::ImageFormat::Png) => return ImageFormat::Png,
        Ok(image::ImageFormat::Bmp) => return ImageFormat::Bmp,
        _ => {}
    }
    let is_tga = file_name
        .rsplit_once('.')
        .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("tga"));
    if is_tga {
        ImageFormat::Tga
    } else {
        ImageFormat::Other
    }
}

/// Width and height from the headers of formats this codec does not decode.
fn header_dimensions(format: ImageFormat, bytes: &[u8]) -> Option<(u32, u32)> {
    let u16_at = |o: usize| {
        bytes
            .get(o..o + 2)
            .map(|b| u32::from(u16::from_le_bytes([b[0], b[1]])))
    };
    let u32_at = |o: usize| {
        bytes
            .get(o..o + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    match format {
        // Logical screen size.
        ImageFormat::Gif => Some((u16_at(6)?, u16_at(8)?)),
        // DDS_HEADER: dwHeight at +12, dwWidth at +16 (after the 4-byte magic).
        ImageFormat::Dds => Some((u32_at(16)?, u32_at(12)?)),
        // 18-byte TGA header: image type at 2, width at 12, height at 14.
        ImageFormat::Tga => {
            let image_type = *bytes.get(2)?;
            [1, 2, 3, 9, 10, 11]
                .contains(&image_type)
                .then_some((u16_at(12)?, u16_at(14)?))
        }
        _ => None,
    }
    .filter(|&(w, h)| w > 0 && h > 0)
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

    fn probe(&self, bytes: &[u8], file_name: &str) -> Result<ImageProbe, ImageCodecError> {
        let format = detect(bytes, file_name);
        let dimensions = match crate_format(format) {
            Some(f) => Some(ImageReader::with_format(Cursor::new(bytes), f).into_dimensions()?),
            None => header_dimensions(format, bytes),
        };
        Ok(ImageProbe { format, dimensions })
    }

    fn decode_fit(
        &self,
        bytes: &[u8],
        max_w: u32,
        max_h: u32,
    ) -> Result<CodecImage, ImageCodecError> {
        let format = crate_format(detect(bytes, "")).ok_or(ImageCodecError::Unsupported)?;
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
