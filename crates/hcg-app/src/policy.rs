//! Resize policy: decides per image whether and how to shrink it (issue #7). Pure logic over the
//! [`ImageCodec`] port, so it is tested with a fake codec.

use crate::ports::{ImageCodec, ImageFormat, ImageProbe, OutputFormat};

/// User-adjustable limits. `Default` gives the documented defaults.
#[derive(Debug, Clone, PartialEq)]
pub struct ResizePolicy {
    /// Output must fit within `max_width`×`max_height` (default 1920×1080).
    pub max_width: u32,
    pub max_height: u32,
    /// Target file size (default 1 MiB).
    pub max_bytes: u64,
    /// First JPEG quality tried (default 90).
    pub jpeg_quality: u8,
    /// Lowest JPEG quality tried before giving up on the size target (default 50).
    pub min_jpeg_quality: u8,
    /// Quality decrement between tries (default 5).
    pub quality_step: u8,
    /// Images with more pixels than this are not decoded (default 100 megapixels).
    pub max_pixels: u64,
}

impl Default for ResizePolicy {
    fn default() -> Self {
        Self {
            max_width: 1920,
            max_height: 1080,
            max_bytes: 1 << 20,
            jpeg_quality: 90,
            min_jpeg_quality: 50,
            quality_step: 5,
            max_pixels: 100_000_000,
        }
    }
}

/// A policy value that makes no sense.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PolicyError {
    #[error("maximum width and height must be at least 1")]
    ZeroDimension,
    #[error("size limit must be at least 1 byte")]
    ZeroSizeLimit,
    #[error("JPEG quality must be 1-100, got {0}")]
    InvalidQuality(u8),
    #[error("minimum JPEG quality {min} is above the starting quality {start}")]
    MinAboveStart { min: u8, start: u8 },
    #[error("quality step must be at least 1")]
    ZeroStep,
}

impl ResizePolicy {
    pub fn validate(&self) -> Result<(), PolicyError> {
        if self.max_width == 0 || self.max_height == 0 || self.max_pixels == 0 {
            return Err(PolicyError::ZeroDimension);
        }
        if self.max_bytes == 0 {
            return Err(PolicyError::ZeroSizeLimit);
        }
        for q in [self.jpeg_quality, self.min_jpeg_quality] {
            if !(1..=100).contains(&q) {
                return Err(PolicyError::InvalidQuality(q));
            }
        }
        if self.min_jpeg_quality > self.jpeg_quality {
            return Err(PolicyError::MinAboveStart {
                min: self.min_jpeg_quality,
                start: self.jpeg_quality,
            });
        }
        if self.quality_step == 0 {
            return Err(PolicyError::ZeroStep);
        }
        Ok(())
    }
}

/// Why an image is left as it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeepReason {
    /// Already within the size and resolution limits.
    WithinLimits,
    /// A format ez-CGER loads but this tool does not shrink (GIF may be animated, DDS is a GPU
    /// texture, TGA is rare). The report shows its size only.
    NotResized(ImageFormat),
    /// A format ez-CGER cannot load (WebP, HEIC, unknown). Worth a warning if the hcg uses it.
    NotSupportedByApp(ImageFormat),
    /// The file could not be read as an image.
    Undecodable(String),
    /// Too many pixels to decode safely.
    TooManyPixels { width: u32, height: u32, limit: u64 },
    /// Encoding failed.
    EncodeFailed(String),
    /// Re-encoding with the same format and size would not save at least
    /// [`MIN_SAVING_PERCENT`] of the original.
    NoGain,
}

/// What analyze decides for an image from its header alone (no decoding).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlannedAction {
    Keep(KeepReason),
    /// Decode and re-encode; `target` is the size after fitting (may equal the original size
    /// when only the file size is over the limit).
    Shrink {
        target: (u32, u32),
    },
}

/// A re-encode with the same format and size must save at least this share of the original
/// size, or the original is kept (avoids generation loss for marginal gains; makes a second run
/// over the tool's own output a no-op).
pub const MIN_SAVING_PERCENT: u64 = 10;

/// Decides from header facts alone whether an image will be shrunk (shared by analyze and
/// [`optimize_image`]).
pub fn plan_image(probe: &ImageProbe, byte_len: u64, policy: &ResizePolicy) -> PlannedAction {
    if !probe.format.supported_by_app() {
        return PlannedAction::Keep(KeepReason::NotSupportedByApp(probe.format));
    }
    if !probe.format.resizable() {
        return PlannedAction::Keep(KeepReason::NotResized(probe.format));
    }
    let Some((width, height)) = probe.dimensions else {
        return PlannedAction::Keep(KeepReason::Undecodable("image size is unknown".into()));
    };
    if u64::from(width) * u64::from(height) > policy.max_pixels {
        return PlannedAction::Keep(KeepReason::TooManyPixels {
            width,
            height,
            limit: policy.max_pixels,
        });
    }
    let fits = width <= policy.max_width && height <= policy.max_height;
    if fits && byte_len <= policy.max_bytes {
        return PlannedAction::Keep(KeepReason::WithinLimits);
    }
    PlannedAction::Shrink {
        target: fit_within(width, height, policy.max_width, policy.max_height),
    }
}

/// A converted image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Converted {
    /// The encoded image. apply moves these bytes into the output folder, leaving this empty;
    /// `byte_len` keeps the size.
    pub bytes: Vec<u8>,
    pub byte_len: u64,
    pub format: OutputFormat,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageAction {
    Unchanged(KeepReason),
    Converted(Converted),
}

/// Something the report should point out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageWarning {
    /// The result (or the kept original) is still above the size limit.
    OverSizeLimit { bytes: u64, limit: u64 },
}

/// What happened to one image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageOutcome {
    /// Header facts of the original, if it could be read.
    pub probe: Option<ImageProbe>,
    pub action: ImageAction,
    pub warnings: Vec<ImageWarning>,
}

impl ImageOutcome {
    fn unchanged(probe: Option<ImageProbe>, reason: KeepReason) -> Self {
        Self {
            probe,
            action: ImageAction::Unchanged(reason),
            warnings: Vec::new(),
        }
    }
}

/// Applies the policy to one image file (contracts in issue #7). `file_name` is only used to
/// recognise TGA. Never fails: problems become [`KeepReason`]s and [`ImageWarning`]s.
pub fn optimize_image<C: ImageCodec>(
    codec: &C,
    file_name: &str,
    bytes: &[u8],
    policy: &ResizePolicy,
) -> ImageOutcome {
    let probe = match codec.probe(bytes, file_name) {
        Ok(p) => p,
        Err(e) => return ImageOutcome::unchanged(None, KeepReason::Undecodable(e.to_string())),
    };
    let keep = |reason| ImageOutcome::unchanged(Some(probe), reason);
    let original_len = bytes.len() as u64;
    if let PlannedAction::Keep(reason) = plan_image(&probe, original_len, policy) {
        return keep(reason);
    }
    // plan_image only plans a shrink when the size is known.
    let Some((width, height)) = probe.dimensions else {
        return keep(KeepReason::Undecodable("image size is unknown".into()));
    };

    let image = match codec.decode_fit(bytes, policy.max_width, policy.max_height) {
        Ok(image) => image,
        Err(e) => return keep(KeepReason::Undecodable(e.to_string())),
    };
    let (out_w, out_h) = codec.dimensions(&image);
    let mut warnings = Vec::new();

    // Lossy JPEG is fine; only real transparency needs PNG.
    let encoded = if codec.uses_alpha(&image) {
        codec
            .encode(&image, OutputFormat::Png)
            .map(|b| (b, OutputFormat::Png))
    } else {
        encode_jpeg_within(codec, &image, policy)
    };
    let (out, format) = match encoded {
        Ok(result) => result,
        Err(e) => {
            return ImageOutcome {
                probe: Some(probe),
                action: ImageAction::Unchanged(KeepReason::EncodeFailed(e.to_string())),
                warnings,
            };
        }
    };

    let same_format = matches!(
        (probe.format, format),
        (ImageFormat::Jpeg, OutputFormat::Jpeg { .. }) | (ImageFormat::Png, OutputFormat::Png)
    );
    let saves_enough = (out.len() as u64) * 100 <= original_len * (100 - MIN_SAVING_PERCENT);
    if same_format && (out_w, out_h) == (width, height) && !saves_enough {
        warnings.push(ImageWarning::OverSizeLimit {
            bytes: original_len,
            limit: policy.max_bytes,
        });
        return ImageOutcome {
            probe: Some(probe),
            action: ImageAction::Unchanged(KeepReason::NoGain),
            warnings,
        };
    }
    if out.len() as u64 > policy.max_bytes {
        warnings.push(ImageWarning::OverSizeLimit {
            bytes: out.len() as u64,
            limit: policy.max_bytes,
        });
    }
    ImageOutcome {
        probe: Some(probe),
        action: ImageAction::Converted(Converted {
            byte_len: out.len() as u64,
            bytes: out,
            format,
            width: out_w,
            height: out_h,
        }),
        warnings,
    }
}

/// Size of a `width`×`height` image scaled to fit within `max_w`×`max_h`, keeping the aspect
/// ratio (rounded to the nearest pixel, at least 1). Never upscales (contract I2).
pub fn fit_within(width: u32, height: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    if width <= max_w && height <= max_h {
        return (width, height);
    }
    let (w, h, mw, mh) = (
        u64::from(width),
        u64::from(height),
        u64::from(max_w),
        u64::from(max_h),
    );
    let round_div = |n: u64, d: u64| ((n + d / 2) / d).max(1);
    // Width-limited if w/h > mw/mh.
    let (tw, th) = if w * mh > h * mw {
        (mw, round_div(h * mw, w))
    } else {
        (round_div(w * mh, h), mh)
    };
    (
        u32::try_from(tw).unwrap_or(max_w).min(max_w),
        u32::try_from(th).unwrap_or(max_h).min(max_h),
    )
}

/// Tries JPEG qualities from `jpeg_quality` down to `min_jpeg_quality`; returns the first result
/// within the size limit, or the minimum-quality result.
fn encode_jpeg_within<C: ImageCodec>(
    codec: &C,
    image: &C::Image,
    policy: &ResizePolicy,
) -> Result<(Vec<u8>, OutputFormat), C::Error> {
    let mut quality = policy.jpeg_quality;
    loop {
        let format = OutputFormat::Jpeg { quality };
        let bytes = codec.encode(image, format)?;
        if bytes.len() as u64 <= policy.max_bytes || quality <= policy.min_jpeg_quality {
            return Ok((bytes, format));
        }
        quality = quality
            .saturating_sub(policy.quality_step.max(1))
            .max(policy.min_jpeg_quality);
    }
}

/// File name for an image after conversion to `format`: the original name if its extension
/// already matches (`.JPG`, `.jpeg`, `.png`, any case), otherwise the extension is replaced with
/// `.jpg` / `.png` (or appended). Returns `(name, extension_changed)`.
pub fn output_file_name(name: &str, format: OutputFormat) -> (String, bool) {
    let (accepted, new_ext): (&[&str], &str) = match format {
        OutputFormat::Jpeg { .. } => (&["jpg", "jpeg"], "jpg"),
        OutputFormat::Png => (&["png"], "png"),
    };
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, Some(ext)),
        _ => (name, None),
    };
    match ext {
        Some(ext) if accepted.iter().any(|a| ext.eq_ignore_ascii_case(a)) => {
            (name.to_string(), false)
        }
        _ => (format!("{stem}.{new_ext}"), true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Fake image: size and alpha. Encoded size = pixels * quality / 100 / 4 bytes,
    /// or pixels / 2 for PNG, so tests control sizes exactly.
    #[derive(Clone)]
    struct FakeImage {
        w: u32,
        h: u32,
        alpha: bool,
    }

    struct FakeCodec {
        probe: Result<ImageProbe, String>,
        image: FakeImage,
        decode_fails: bool,
        decodes: RefCell<u32>,
        qualities: RefCell<Vec<u8>>,
    }

    #[derive(Debug, thiserror::Error)]
    #[error("{0}")]
    struct FakeError(String);

    impl FakeCodec {
        fn new(format: ImageFormat, w: u32, h: u32) -> Self {
            Self {
                probe: Ok(ImageProbe {
                    format,
                    dimensions: Some((w, h)),
                }),
                image: FakeImage { w, h, alpha: false },
                decode_fails: false,
                decodes: RefCell::new(0),
                qualities: RefCell::new(Vec::new()),
            }
        }
    }

    impl ImageCodec for FakeCodec {
        type Image = FakeImage;
        type Error = FakeError;

        fn probe(&self, _: &[u8], _: &str) -> Result<ImageProbe, FakeError> {
            self.probe.clone().map_err(FakeError)
        }

        fn decode_fit(&self, _: &[u8], max_w: u32, max_h: u32) -> Result<FakeImage, FakeError> {
            *self.decodes.borrow_mut() += 1;
            if self.decode_fails {
                return Err(FakeError("broken".into()));
            }
            let mut img = self.image.clone();
            (img.w, img.h) = fit_within(img.w, img.h, max_w, max_h);
            Ok(img)
        }

        fn dimensions(&self, img: &FakeImage) -> (u32, u32) {
            (img.w, img.h)
        }

        fn uses_alpha(&self, img: &FakeImage) -> bool {
            img.alpha
        }

        fn encode(&self, img: &FakeImage, format: OutputFormat) -> Result<Vec<u8>, FakeError> {
            let pixels = img.w as usize * img.h as usize;
            let len = match format {
                OutputFormat::Jpeg { quality } => {
                    self.qualities.borrow_mut().push(quality);
                    pixels * quality as usize / 100 / 4
                }
                OutputFormat::Png => pixels / 2,
            };
            Ok(vec![0; len])
        }
    }

    fn run(codec: &FakeCodec, input_len: usize) -> ImageOutcome {
        optimize_image(codec, "x", &vec![0; input_len], &ResizePolicy::default())
    }

    fn converted(o: &ImageOutcome) -> &Converted {
        match &o.action {
            ImageAction::Converted(c) => c,
            other => panic!("expected conversion, got {other:?}"),
        }
    }

    #[test]
    fn i1_within_limits_is_kept_without_decoding() {
        let codec = FakeCodec::new(ImageFormat::Png, 1920, 1080);
        let o = run(&codec, 1 << 20);
        assert_eq!(o.action, ImageAction::Unchanged(KeepReason::WithinLimits));
        assert_eq!(*codec.decodes.borrow(), 0);
    }

    #[test]
    fn i2_large_photo_is_fitted_and_decoded_once() {
        let codec = FakeCodec::new(ImageFormat::Jpeg, 6000, 4000);
        let o = run(&codec, 23_000_000);
        let c = converted(&o);
        assert_eq!((c.width, c.height), (1620, 1080));
        assert_eq!(*codec.decodes.borrow(), 1);
        // 1620*1080*90/100/4 = 393,660 bytes: fits at the first quality.
        assert_eq!(c.format, OutputFormat::Jpeg { quality: 90 });
        assert!(o.warnings.is_empty());
    }

    #[test]
    fn i3_alpha_stays_png() {
        let mut codec = FakeCodec::new(ImageFormat::Png, 4000, 2000);
        codec.image.alpha = true;
        let o = run(&codec, 3_000_000);
        assert_eq!(converted(&o).format, OutputFormat::Png);
    }

    #[test]
    fn i4_quality_steps_down_until_within_limit() {
        // 1920x1080 = 2,073,600 px. JPEG bytes = px*q/400; <= 1 MiB needs q <= 202 -> q 90 is
        // 466k. Use a smaller limit to force steps: 300,000 bytes needs q <= 57 -> 55.
        let codec = FakeCodec::new(ImageFormat::Png, 1920, 1080);
        let policy = ResizePolicy {
            max_bytes: 300_000,
            ..ResizePolicy::default()
        };
        let o = optimize_image(&codec, "x", &vec![0; 6_000_000], &policy);
        assert_eq!(converted(&o).format, OutputFormat::Jpeg { quality: 55 });
        assert_eq!(*codec.qualities.borrow(), [90, 85, 80, 75, 70, 65, 60, 55]);
        assert!(o.warnings.is_empty());
    }

    #[test]
    fn i4_minimum_quality_with_warning_when_limit_unreachable() {
        let codec = FakeCodec::new(ImageFormat::Png, 1920, 1080);
        let policy = ResizePolicy {
            max_bytes: 100_000,
            ..ResizePolicy::default()
        };
        let o = optimize_image(&codec, "x", &vec![0; 6_000_000], &policy);
        assert_eq!(converted(&o).format, OutputFormat::Jpeg { quality: 50 });
        assert_eq!(
            o.warnings,
            [ImageWarning::OverSizeLimit {
                bytes: 259_200,
                limit: 100_000
            }]
        );
    }

    #[test]
    fn i4_png_over_limit_warns() {
        let mut codec = FakeCodec::new(ImageFormat::Png, 3840, 2160);
        codec.image.alpha = true;
        let policy = ResizePolicy {
            max_bytes: 1_000_000,
            ..ResizePolicy::default()
        };
        let o = optimize_image(&codec, "x", &vec![0; 9_000_000], &policy);
        // Fitted to 1920x1080; fake PNG = 2,073,600 / 2 bytes.
        assert_eq!(converted(&o).format, OutputFormat::Png);
        assert_eq!(
            o.warnings,
            [ImageWarning::OverSizeLimit {
                bytes: 1_036_800,
                limit: 1_000_000
            }]
        );
    }

    #[test]
    fn i6_unsupported_and_broken_files_are_kept() {
        for format in [ImageFormat::Gif, ImageFormat::Dds, ImageFormat::Tga] {
            let codec = FakeCodec::new(format, 4000, 4000);
            let o = run(&codec, 9_000_000);
            assert_eq!(
                o.action,
                ImageAction::Unchanged(KeepReason::NotResized(format))
            );
            assert_eq!(
                o.probe.and_then(|p| p.dimensions),
                Some((4000, 4000)),
                "size for the report"
            );
            assert_eq!(*codec.decodes.borrow(), 0);
        }
        for format in [ImageFormat::WebP, ImageFormat::Heic, ImageFormat::Other] {
            let codec = FakeCodec::new(format, 4000, 4000);
            assert_eq!(
                run(&codec, 9_000_000).action,
                ImageAction::Unchanged(KeepReason::NotSupportedByApp(format))
            );
        }
        let mut codec = FakeCodec::new(ImageFormat::Jpeg, 4000, 3000);
        codec.probe = Err("not an image".into());
        assert!(matches!(
            run(&codec, 10).action,
            ImageAction::Unchanged(KeepReason::Undecodable(_))
        ));
        let mut codec = FakeCodec::new(ImageFormat::Jpeg, 4000, 3000);
        codec.decode_fails = true;
        assert!(matches!(
            run(&codec, 9_000_000).action,
            ImageAction::Unchanged(KeepReason::Undecodable(_))
        ));
        let codec = FakeCodec::new(ImageFormat::Jpeg, 20_000, 10_000);
        assert_eq!(
            run(&codec, 9_000_000).action,
            ImageAction::Unchanged(KeepReason::TooManyPixels {
                width: 20_000,
                height: 10_000,
                limit: 100_000_000
            })
        );
        assert_eq!(*codec.decodes.borrow(), 0);
    }

    #[test]
    fn opaque_png_becomes_jpeg() {
        let codec = FakeCodec::new(ImageFormat::Png, 1920, 1080);
        let o = run(&codec, 6_000_000);
        assert!(matches!(
            converted(&o).format,
            OutputFormat::Jpeg { quality: 90 }
        ));
    }

    #[test]
    fn i10_no_gain_keeps_original() {
        // A 1920x1080 JPEG of 1.1 MB whose re-encode at the minimum quality is still larger.
        let codec = FakeCodec::new(ImageFormat::Jpeg, 1920, 1080);
        let policy = ResizePolicy {
            max_bytes: 200_000,
            ..ResizePolicy::default()
        };
        let o = optimize_image(&codec, "x", &vec![0; 250_000], &policy);
        assert_eq!(o.action, ImageAction::Unchanged(KeepReason::NoGain));
        assert_eq!(
            o.warnings,
            [ImageWarning::OverSizeLimit {
                bytes: 250_000,
                limit: 200_000
            }]
        );
    }

    #[test]
    fn i10_reencode_must_save_ten_percent() {
        // 1920x1080 JPEG over a 400,000-byte limit: q75 gives 388,800 bytes.
        let codec = FakeCodec::new(ImageFormat::Jpeg, 1920, 1080);
        let policy = ResizePolicy {
            max_bytes: 400_000,
            ..ResizePolicy::default()
        };
        // 388,800 <= 90% of 480,000: accepted.
        let o = optimize_image(&codec, "x", &vec![0; 480_000], &policy);
        assert_eq!(converted(&o).format, OutputFormat::Jpeg { quality: 75 });
        // 388,800 > 90% of 420,000: not worth the generation loss.
        let o = optimize_image(&codec, "x", &vec![0; 420_000], &policy);
        assert_eq!(o.action, ImageAction::Unchanged(KeepReason::NoGain));
    }

    #[test]
    fn plan_image_needs_only_the_header() {
        let probe = |format, w, h| ImageProbe {
            format,
            dimensions: Some((w, h)),
        };
        let p = ResizePolicy::default();
        assert_eq!(
            plan_image(&probe(ImageFormat::Jpeg, 6000, 4000), 23_000_000, &p),
            PlannedAction::Shrink {
                target: (1620, 1080)
            }
        );
        assert_eq!(
            plan_image(&probe(ImageFormat::Png, 1920, 1080), 6_000_000, &p),
            PlannedAction::Shrink {
                target: (1920, 1080)
            }
        );
        assert_eq!(
            plan_image(&probe(ImageFormat::Png, 600, 400), 3_000, &p),
            PlannedAction::Keep(KeepReason::WithinLimits)
        );
        assert_eq!(
            plan_image(&probe(ImageFormat::Gif, 6000, 4000), 9_000_000, &p),
            PlannedAction::Keep(KeepReason::NotResized(ImageFormat::Gif))
        );
        assert_eq!(
            plan_image(&probe(ImageFormat::WebP, 600, 400), 3_000, &p),
            PlannedAction::Keep(KeepReason::NotSupportedByApp(ImageFormat::WebP))
        );
    }

    #[test]
    fn bmp_within_resolution_but_large_becomes_jpeg() {
        let codec = FakeCodec::new(ImageFormat::Bmp, 1920, 1080);
        let o = run(&codec, 6_220_854);
        assert_eq!((converted(&o).width, converted(&o).height), (1920, 1080));
        assert!(matches!(
            converted(&o).format,
            OutputFormat::Jpeg { quality: 90 }
        ));
    }

    #[test]
    fn i2_fit_within() {
        assert_eq!(fit_within(6000, 4000, 1920, 1080), (1620, 1080));
        assert_eq!(fit_within(4000, 3000, 1920, 1080), (1440, 1080));
        assert_eq!(fit_within(3840, 2160, 1920, 1080), (1920, 1080));
        assert_eq!(fit_within(4000, 1000, 1920, 1080), (1920, 480));
        assert_eq!(fit_within(1000, 4000, 1920, 1080), (270, 1080));
        assert_eq!(fit_within(1920, 1080, 1920, 1080), (1920, 1080));
        assert_eq!(
            fit_within(800, 600, 1920, 1080),
            (800, 600),
            "never upscaled"
        );
        assert_eq!(fit_within(100_000, 1, 1920, 1080), (1920, 1));
        // Aspect ratio within one pixel for many sizes.
        for w in (100..9000).step_by(37) {
            for h in (100..9000).step_by(53) {
                let (tw, th) = fit_within(w, h, 1920, 1080);
                assert!(tw <= 1920 && th <= 1080 && tw <= w && th <= h);
                let ideal_h = th as f64 * w as f64 / h as f64;
                assert!(
                    (tw as f64 - ideal_h).abs() <= 1.0
                        || (th as f64 - tw as f64 * h as f64 / w as f64).abs() <= 1.0
                );
            }
        }
    }

    #[test]
    fn i5_output_file_names() {
        let jpeg = OutputFormat::Jpeg { quality: 90 };
        assert_eq!(output_file_name("bg.JPG", jpeg), ("bg.JPG".into(), false));
        assert_eq!(output_file_name("bg.jpeg", jpeg), ("bg.jpeg".into(), false));
        assert_eq!(
            output_file_name("noise.png", jpeg),
            ("noise.jpg".into(), true)
        );
        assert_eq!(
            output_file_name("old.bmp", OutputFormat::Png),
            ("old.png".into(), true)
        );
        assert_eq!(
            output_file_name("logo.PNG", OutputFormat::Png),
            ("logo.PNG".into(), false)
        );
        assert_eq!(output_file_name("a.b.bmp", jpeg), ("a.b.jpg".into(), true));
        assert_eq!(output_file_name("noext", jpeg), ("noext.jpg".into(), true));
        assert_eq!(
            output_file_name(".hidden", jpeg),
            (".hidden.jpg".into(), true)
        );
    }

    #[test]
    fn policy_validation() {
        assert_eq!(ResizePolicy::default().validate(), Ok(()));
        let bad = |p: ResizePolicy| p.validate().expect_err("invalid");
        let d = ResizePolicy::default;
        assert_eq!(
            bad(ResizePolicy {
                max_width: 0,
                ..d()
            }),
            PolicyError::ZeroDimension
        );
        assert_eq!(
            bad(ResizePolicy {
                max_bytes: 0,
                ..d()
            }),
            PolicyError::ZeroSizeLimit
        );
        assert_eq!(
            bad(ResizePolicy {
                jpeg_quality: 101,
                ..d()
            }),
            PolicyError::InvalidQuality(101)
        );
        assert_eq!(
            bad(ResizePolicy {
                min_jpeg_quality: 95,
                ..d()
            }),
            PolicyError::MinAboveStart { min: 95, start: 90 }
        );
        assert_eq!(
            bad(ResizePolicy {
                quality_step: 0,
                ..d()
            }),
            PolicyError::ZeroStep
        );
    }
}
