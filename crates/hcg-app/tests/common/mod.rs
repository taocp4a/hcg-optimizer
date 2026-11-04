//! Shared test support: a fake codec and sample-bundle builders.
#![allow(dead_code)]

use std::cell::Cell;
use std::path::Path;

use hcg_app::bundle::{Bundle, BundleFile};
use hcg_app::ports::{ImageCodec, ImageFormat, ImageProbe, OutputFormat};
use hcg_format::{ImageSource, parse};

/// Fake image files are text: `FAKE <png|jpg|bmp> <w> <h> <alpha 0|1>`, padded to a size.
pub fn fake(format: &str, w: u32, h: u32, alpha: bool, size: usize) -> Vec<u8> {
    let mut b = format!("FAKE {format} {w} {h} {}", u8::from(alpha)).into_bytes();
    b.resize(size.max(b.len()), b' ');
    b
}

pub struct FakeImage {
    pub w: u32,
    pub h: u32,
    pub alpha: bool,
}

#[derive(Default)]
pub struct FakeCodec {
    pub decodes: Cell<u32>,
}

#[derive(Debug, thiserror::Error)]
#[error("not a fake image")]
pub struct NotFake;

pub fn parse_fake(bytes: &[u8]) -> Result<(ImageFormat, u32, u32, bool), NotFake> {
    let text = std::str::from_utf8(bytes).map_err(|_| NotFake)?;
    let mut it = text.split_whitespace();
    if it.next() != Some("FAKE") {
        return Err(NotFake);
    }
    let format = match it.next() {
        Some("png") => ImageFormat::Png,
        Some("jpg") => ImageFormat::Jpeg,
        Some("bmp") => ImageFormat::Bmp,
        Some("gif") => ImageFormat::Gif,
        _ => return Err(NotFake),
    };
    let mut num = || it.next().and_then(|n| n.parse::<u32>().ok()).ok_or(NotFake);
    Ok((format, num()?, num()?, num()? == 1))
}

impl ImageCodec for FakeCodec {
    type Image = FakeImage;
    type Error = NotFake;

    fn probe(&self, bytes: &[u8], _: &str) -> Result<ImageProbe, NotFake> {
        let (format, w, h, _) = parse_fake(bytes)?;
        Ok(ImageProbe {
            format,
            dimensions: Some((w, h)),
        })
    }

    fn decode_fit(&self, bytes: &[u8], max_w: u32, max_h: u32) -> Result<FakeImage, NotFake> {
        self.decodes.set(self.decodes.get() + 1);
        let (_, w, h, alpha) = parse_fake(bytes)?;
        let (w, h) = hcg_app::policy::fit_within(w, h, max_w, max_h);
        Ok(FakeImage { w, h, alpha })
    }

    fn dimensions(&self, image: &FakeImage) -> (u32, u32) {
        (image.w, image.h)
    }

    fn uses_alpha(&self, image: &FakeImage) -> bool {
        image.alpha
    }

    fn encode(&self, image: &FakeImage, format: OutputFormat) -> Result<Vec<u8>, NotFake> {
        let (name, size) = match format {
            OutputFormat::Jpeg { quality } => (
                "jpg",
                image.w as usize * image.h as usize * quality as usize / 1000,
            ),
            OutputFormat::Png => ("png", image.w as usize * image.h as usize / 4),
        };
        Ok(fake(name, image.w, image.h, image.alpha, size))
    }
}

pub fn sample_hcg(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../project/samples")
            .join(name),
    )
    .expect("sample")
}

/// A bundle from a sample hcg whose folder holds the given fake file for each referenced
/// original file name (e.g. "mark_1.png").
pub fn bundle(hcg_name: &str, files: &[(&str, Vec<u8>)]) -> Bundle {
    let hcg = sample_hcg(hcg_name);
    let parsed = parse(&hcg).expect("parse");
    let mut images: Vec<BundleFile> = Vec::new();
    for image in parsed.images() {
        let Some(folder_name) = image.folder_file_name else {
            continue;
        };
        let original = folder_name[8..].to_string();
        if images.iter().any(|f| f.name == folder_name) {
            continue;
        }
        if let Some((_, bytes)) = files.iter().find(|(n, _)| *n == original) {
            images.push(BundleFile {
                name: folder_name,
                bytes: bytes.clone(),
            });
        }
    }
    let stem = hcg_name.trim_end_matches(".hcg").to_string();
    Bundle {
        folder_path: Some(stem.clone()),
        hcg_path: hcg_name.to_string(),
        stem,
        hcg,
        images,
    }
}

pub fn path_of(source: &ImageSource) -> &str {
    match source {
        ImageSource::Path(p) => p,
        other => panic!("not a path: {other:?}"),
    }
}
