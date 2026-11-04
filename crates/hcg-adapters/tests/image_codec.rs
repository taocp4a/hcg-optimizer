//! Image processing with the real codec (issue #7, contracts I1–I10).

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::Instant;

use hcg_adapters::image_codec::PureRustCodec;
use hcg_app::policy::{
    Converted, ImageAction, ImageOutcome, ImageWarning, KeepReason, ResizePolicy, optimize_image,
    output_file_name,
};
use hcg_app::ports::{ImageFormat, OutputFormat};
use image::codecs::jpeg::JpegEncoder;
use image::{
    DynamicImage, ExtendedColorType, GenericImageView, ImageDecoder, ImageEncoder, ImageReader,
    Rgb, RgbImage, Rgba, RgbaImage,
};

const MIB: u64 = 1 << 20;

fn sample(path: &str) -> Vec<u8> {
    let p: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../project/samples")
        .join(path);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn optimize(bytes: &[u8]) -> ImageOutcome {
    optimize_named("image", bytes)
}

fn optimize_named(name: &str, bytes: &[u8]) -> ImageOutcome {
    optimize_image(
        &PureRustCodec::default(),
        name,
        bytes,
        &ResizePolicy::default(),
    )
}

fn converted(o: &ImageOutcome) -> &Converted {
    match &o.action {
        ImageAction::Converted(c) => c,
        other => panic!("expected conversion, got {other:?}"),
    }
}

fn decode(bytes: &[u8]) -> (image::ImageFormat, DynamicImage) {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .expect("guess");
    let format = reader.format().expect("format");
    (format, reader.decode().expect("decode"))
}

/// Deterministic pseudo-random noise, so tests do not depend on a random source.
fn noise(seed: u32) -> impl FnMut() -> u8 {
    let mut state = seed;
    move || {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (state >> 24) as u8
    }
}

fn png(img: &DynamicImage) -> Vec<u8> {
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
        .expect("png");
    out
}

/// I2/I4/I5 on the 6000×4000 sample photo; also reports the time taken.
#[test]
fn large_photo_is_fitted_under_the_limit() {
    let bytes = sample("speedtest/D727E722bg_6000x4000_photo.jpg");
    let start = Instant::now();
    let o = optimize(&bytes);
    eprintln!("6000x4000 photo: {:?}", start.elapsed());
    let c = converted(&o);
    assert_eq!((c.width, c.height), (1620, 1080));
    assert!(
        matches!(c.format, OutputFormat::Jpeg { .. }),
        "{:?}",
        c.format
    );
    assert!(c.bytes.len() as u64 <= MIB, "{} bytes", c.bytes.len());
    assert!(o.warnings.is_empty(), "{:?}", o.warnings);
    let (format, img) = decode(&c.bytes);
    assert_eq!(
        (format, img.dimensions()),
        (image::ImageFormat::Jpeg, (1620, 1080))
    );
    assert_eq!(
        output_file_name("bg_6000x4000_photo.jpg", c.format),
        ("bg_6000x4000_photo.jpg".into(), false)
    );
    eprintln!("  -> {} bytes, {:?}", c.bytes.len(), c.format);
}

#[test]
fn twelve_mb_jpeg_is_fitted() {
    let o = optimize(&sample("layered/0D373705big_0.jpg"));
    let c = converted(&o);
    assert_eq!((c.width, c.height), (1440, 1080));
    assert!(c.bytes.len() as u64 <= MIB);
}

/// A 1920×1080 PNG without alpha but 6 MB: same size, converted to JPEG, extension changes.
#[test]
fn opaque_png_over_the_size_limit_becomes_jpeg() {
    let o = optimize(&sample("speedtest/2D00575Abg_1080_noise.png"));
    let c = converted(&o);
    assert_eq!((c.width, c.height), (1920, 1080));
    let OutputFormat::Jpeg { quality } = c.format else {
        panic!("{:?}", c.format)
    };
    let within = c.bytes.len() as u64 <= MIB;
    // Pure noise is the worst case for JPEG: either it fits, or the minimum quality is used
    // with a warning.
    assert!(
        within || (quality == 50 && matches!(o.warnings[..], [ImageWarning::OverSizeLimit { .. }]))
    );
    assert_eq!(
        output_file_name("bg_1080_noise.png", c.format),
        ("bg_1080_noise.jpg".into(), true)
    );
    eprintln!("noise png -> {} bytes at quality {quality}", c.bytes.len());
}

/// I1: every image already within the limits is left alone.
#[test]
fn small_images_are_kept() {
    for path in [
        "layered/7CC25255layer_0.png",
        "layered/28BD2D7Emark_1.png",
        "speedtest/D4E3A7EAbg_1080_small.jpg",
        "v11_two_images/74EB6CEBmark_2.png",
    ] {
        let o = optimize(&sample(path));
        assert_eq!(
            o.action,
            ImageAction::Unchanged(KeepReason::WithinLimits),
            "{path}"
        );
        assert!(o.warnings.is_empty(), "{path}");
    }
}

/// BMP without alpha is converted to JPEG.
#[test]
fn bmp_becomes_jpeg() {
    let mut rnd = noise(1);
    let img = RgbImage::from_fn(2400, 1350, |x, y| {
        Rgb([(x % 256) as u8, (y % 256) as u8, rnd() / 4])
    });
    let mut bmp = Vec::new();
    DynamicImage::ImageRgb8(img)
        .write_to(&mut Cursor::new(&mut bmp), image::ImageFormat::Bmp)
        .expect("bmp");
    let o = optimize(&bmp);
    assert_eq!(o.probe.expect("probe").format, ImageFormat::Bmp);
    let c = converted(&o);
    assert_eq!((c.width, c.height), (1920, 1080));
    assert!(matches!(c.format, OutputFormat::Jpeg { quality: 90 }));
    assert_eq!(
        output_file_name("old.bmp", c.format),
        ("old.jpg".into(), true)
    );
}

/// I3: real transparency stays PNG and survives the resize.
#[test]
fn alpha_png_stays_png() {
    let img = RgbaImage::from_fn(3000, 2000, |x, y| {
        let a = if (x / 100 + y / 100) % 2 == 0 {
            255
        } else {
            120
        };
        Rgba([200, (x % 256) as u8, (y % 256) as u8, a])
    });
    let o = optimize(&png(&DynamicImage::ImageRgba8(img)));
    let c = converted(&o);
    assert_eq!(
        (c.width, c.height, c.format),
        (1620, 1080, OutputFormat::Png)
    );
    let (format, out) = decode(&c.bytes);
    assert_eq!(format, image::ImageFormat::Png);
    assert!(out.to_rgba8().pixels().any(|p| p.0[3] < 255), "alpha lost");
}

/// Lossy JPEG is fine: an opaque image with a flat background becomes JPEG (no colour-key guard).
#[test]
fn opaque_png_with_flat_background_becomes_jpeg() {
    let img = RgbImage::from_fn(2400, 1350, |x, y| {
        if (600..1800).contains(&x) && (300..1050).contains(&y) {
            Rgb([(x % 256) as u8, (y % 256) as u8, 200])
        } else {
            Rgb([0, 0, 16])
        }
    });
    let o = optimize(&png(&DynamicImage::ImageRgb8(img)));
    assert!(matches!(converted(&o).format, OutputFormat::Jpeg { .. }));
    assert!(o.warnings.is_empty());
}

/// I8: the EXIF orientation of the source is carried over; pixels stay in stored orientation.
#[test]
fn exif_orientation_is_kept() {
    let img = RgbImage::from_fn(4000, 3000, |x, y| {
        Rgb([(x / 16) as u8, (y / 12) as u8, 128])
    });
    let mut jpeg = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut jpeg, 95);
    let mut tiff = b"II*\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0".to_vec();
    tiff.extend_from_slice(&[6, 0, 0, 0, 0, 0, 0, 0]); // orientation 6 (rotate 90), padding, no next IFD
    encoder.set_exif_metadata(tiff).expect("exif");
    encoder
        .write_image(img.as_raw(), 4000, 3000, ExtendedColorType::Rgb8)
        .expect("jpeg");

    let o = optimize(&jpeg);
    let c = converted(&o);
    assert_eq!((c.width, c.height), (1440, 1080), "stored orientation kept");
    let mut decoder = ImageReader::new(Cursor::new(&c.bytes))
        .with_guessed_format()
        .expect("guess")
        .into_decoder()
        .expect("decoder");
    assert_eq!(
        decoder.orientation().expect("orientation"),
        image::metadata::Orientation::Rotate90
    );
}

/// I6: app formats this tool does not shrink keep their size for the report; formats the app
/// cannot load are flagged; nothing panics.
#[test]
fn other_formats_are_kept() {
    let gif = b"GIF89a\x40\x01\xf0\x00\x00\x00\x00;".to_vec(); // 320x240
    let mut dds = b"DDS \x7c\0\0\0\x07\x10\0\0".to_vec();
    dds.extend_from_slice(&480u32.to_le_bytes()); // height
    dds.extend_from_slice(&640u32.to_le_bytes()); // width
    dds.resize(128, 0);
    let mut tga = vec![0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    tga.extend_from_slice(&800u16.to_le_bytes());
    tga.extend_from_slice(&600u16.to_le_bytes());
    tga.extend_from_slice(&[32, 8]);
    for (name, bytes, format, size) in [
        ("anim.gif", gif, ImageFormat::Gif, (320, 240)),
        ("tex.dds", dds, ImageFormat::Dds, (640, 480)),
        ("logo.TGA", tga, ImageFormat::Tga, (800, 600)),
    ] {
        let o = optimize_named(name, &bytes);
        assert_eq!(
            o.action,
            ImageAction::Unchanged(KeepReason::NotResized(format)),
            "{name}"
        );
        assert_eq!(o.probe.and_then(|p| p.dimensions), Some(size), "{name}");
    }
    let webp = b"RIFF\x24\0\0\0WEBPVP8 ".to_vec();
    let heic = b"\0\0\0\x18ftypheic\0\0\0\0mif1heic".to_vec();
    for (name, bytes, format) in [
        // Detected by signature even with a misleading extension.
        ("photo.jpg", webp, ImageFormat::WebP),
        ("IMG_0001.HEIC", heic, ImageFormat::Heic),
        ("notes.tga.txt", b"hello".to_vec(), ImageFormat::Other),
    ] {
        assert_eq!(
            optimize_named(name, &bytes).action,
            ImageAction::Unchanged(KeepReason::NotSupportedByApp(format)),
            "{name}"
        );
    }
    // A truncated photo: the header is readable but decoding fails.
    let photo = sample("layered/0D373705big_0.jpg");
    let o = optimize(&photo[..photo.len() / 2]);
    assert!(
        matches!(
            o.action,
            ImageAction::Unchanged(KeepReason::Undecodable(_)) | ImageAction::Converted(_)
        ),
        "{:?}",
        o.action
    );
    for cut in [0, 1, 4, 16, 100, 1000] {
        let _ = optimize(&photo[..cut]);
    }
}

#[test]
fn pixel_limit_is_respected() {
    let policy = ResizePolicy {
        max_pixels: 1_000_000,
        ..ResizePolicy::default()
    };
    let o = optimize_image(
        &PureRustCodec::default(),
        "big_0.jpg",
        &sample("layered/0D373705big_0.jpg"),
        &policy,
    );
    assert_eq!(
        o.action,
        ImageAction::Unchanged(KeepReason::TooManyPixels {
            width: 4000,
            height: 3000,
            limit: 1_000_000
        })
    );
}

/// I7: the same input gives the same output.
#[test]
fn output_is_deterministic() {
    let bytes = sample("layered/2AE57E93big_1.jpg");
    assert_eq!(
        converted(&optimize(&bytes)).bytes,
        converted(&optimize(&bytes)).bytes
    );
}
