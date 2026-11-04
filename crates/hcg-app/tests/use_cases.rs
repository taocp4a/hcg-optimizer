//! analyze/apply/run with a fake codec over real sample hcg files (issue #9, contracts A1–A11).

use std::cell::Cell;
use std::path::Path;

use hcg_app::analyze::analyze;
use hcg_app::apply::{ApplyFailure, Cancelled, apply};
use hcg_app::bundle::{Bundle, BundleFile};
use hcg_app::package::{InputFile, NameEncoding};
use hcg_app::policy::{ImageAction, KeepReason, PlannedAction, ResizePolicy};
use hcg_app::ports::{ImageCodec, ImageFormat, ImageProbe, NoProgress, OutputFormat, Progress};
use hcg_app::run::{plan_run, run};
use hcg_format::{ImageSource, PageTransition, TransitionKind, parse, subfolder_file_name};

/// Fake image files are text: `FAKE <png|jpg|bmp> <w> <h> <alpha 0|1>`, padded to a size.
fn fake(format: &str, w: u32, h: u32, alpha: bool, size: usize) -> Vec<u8> {
    let mut b = format!("FAKE {format} {w} {h} {}", u8::from(alpha)).into_bytes();
    b.resize(size.max(b.len()), b' ');
    b
}

struct FakeImage {
    w: u32,
    h: u32,
    alpha: bool,
}

#[derive(Default)]
struct FakeCodec {
    decodes: Cell<u32>,
}

#[derive(Debug, thiserror::Error)]
#[error("not a fake image")]
struct NotFake;

fn parse_fake(bytes: &[u8]) -> Result<(ImageFormat, u32, u32, bool), NotFake> {
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

fn sample_hcg(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../project/samples")
            .join(name),
    )
    .expect("sample")
}

/// A bundle from a sample hcg whose folder holds the given fake file for each referenced
/// original file name (e.g. "mark_1.png").
fn bundle(hcg_name: &str, files: &[(&str, Vec<u8>)]) -> Bundle {
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

fn path_of(source: &ImageSource) -> &str {
    match source {
        ImageSource::Path(p) => p,
        other => panic!("not a path: {other:?}"),
    }
}

/// A8 + A4 + plan contents on speedtest (the photo is referenced twice: global layer and page 5).
#[test]
fn analyze_plans_without_decoding() {
    let codec = FakeCodec::default();
    let b = bundle(
        "speedtest.hcg",
        &[
            (
                "bg_6000x4000_photo.jpg",
                fake("jpg", 6000, 4000, false, 23_000_000),
            ),
            (
                "bg_1080_noise.png",
                fake("png", 1920, 1080, false, 6_000_000),
            ),
            ("bg_1080_small.jpg", fake("jpg", 1920, 1080, false, 67_000)),
        ],
    );
    let plan = analyze(&codec, &b, &ResizePolicy::default());
    assert_eq!(codec.decodes.get(), 0, "analyze must not decode");
    let files: Vec<_> = plan
        .files
        .iter()
        .map(|f| (&f.name[8..], f.refs.len(), &f.action))
        .collect();
    assert_eq!(
        files,
        [
            (
                "bg_6000x4000_photo.jpg",
                2,
                &PlannedAction::Shrink {
                    target: (1620, 1080)
                }
            ),
            (
                "bg_1080_small.jpg",
                1,
                &PlannedAction::Keep(KeepReason::WithinLimits)
            ),
            (
                "bg_1080_noise.png",
                1,
                &PlannedAction::Shrink {
                    target: (1920, 1080)
                }
            ),
        ]
    );
    assert_eq!(plan.shrink_count(), 2);
    assert!(plan.missing.is_empty() && plan.embedded.is_empty() && plan.unreferenced.is_empty());
}

/// A1–A4: the opaque PNG becomes JPEG; the hcg path, ID and folder name follow; the photo keeps
/// its name with new bytes; the rewritten hcg verifies.
#[test]
fn apply_rewrites_paths_on_extension_change() {
    let codec = FakeCodec::default();
    let b = bundle(
        "speedtest.hcg",
        &[
            (
                "bg_6000x4000_photo.jpg",
                fake("jpg", 6000, 4000, false, 23_000_000),
            ),
            (
                "bg_1080_noise.png",
                fake("png", 1920, 1080, false, 6_000_000),
            ),
            ("bg_1080_small.jpg", fake("jpg", 1920, 1080, false, 67_000)),
        ],
    );
    let policy = ResizePolicy::default();
    let plan = analyze(&codec, &b, &policy);
    let result = apply(&codec, &b, plan, &policy, &mut NoProgress).expect("not cancelled");
    assert_eq!(result.failure, None);
    let out = result.output.expect("output");
    assert_eq!(
        codec.decodes.get(),
        2,
        "the photo is decoded once despite two references"
    );

    let noise = result
        .files
        .iter()
        .find(|f| f.name.ends_with("noise.png"))
        .expect("noise");
    assert_eq!(
        noise.new_path.as_deref(),
        Some("C:\\test\\bg_1080_noise.jpg")
    );
    let photo = result
        .files
        .iter()
        .find(|f| f.name.ends_with("photo.jpg"))
        .expect("photo");
    assert_eq!(
        (photo.new_path.as_deref(), photo.new_name.as_str()),
        (None, photo.name.as_str())
    );

    let parsed = parse(&out.hcg).expect("output parses");
    let names: Vec<&str> = out.images.iter().map(|f| f.name.as_str()).collect();
    for image in parsed.images() {
        let name = image.folder_file_name.expect("path image");
        assert!(
            names.contains(&name.as_str()),
            "{name} missing from the output folder"
        );
        if path_of(&image.source).ends_with("noise.jpg") {
            assert_eq!(
                name,
                subfolder_file_name("C:\\test\\bg_1080_noise.jpg", image.stamp)
            );
        }
    }
    assert!(
        !names.iter().any(|n| n.ends_with("noise.png")),
        "old name removed"
    );
    let photo_bytes = &out
        .images
        .iter()
        .find(|f| f.name == photo.name)
        .expect("photo")
        .bytes;
    assert!(photo_bytes.starts_with(b"FAKE jpg 1620 1080"));
    let small = out
        .images
        .iter()
        .find(|f| f.name.ends_with("small.jpg"))
        .expect("small");
    let original_small = b
        .images
        .iter()
        .find(|f| f.name == small.name)
        .expect("orig");
    assert_eq!(
        small.bytes, original_small.bytes,
        "A6: unchanged file copied as is"
    );
}

/// A4: two objects share one file; both get the same new path and name.
#[test]
fn shared_file_is_renamed_once_for_all_references() {
    let codec = FakeCodec::default();
    let b = bundle(
        "v12_sameimg_twice.hcg",
        &[("mark_1.png", fake("png", 4000, 3000, false, 3_000_000))],
    );
    let policy = ResizePolicy::default();
    let result = apply(
        &codec,
        &b,
        analyze(&codec, &b, &policy),
        &policy,
        &mut NoProgress,
    )
    .expect("ok");
    let out = result.output.expect("output");
    let parsed = parse(&out.hcg).expect("parse");
    let paths: Vec<_> = parsed
        .images()
        .iter()
        .map(|i| path_of(&i.source).to_string())
        .collect();
    assert_eq!(paths, ["C:\\test\\mark_1.jpg", "C:\\test\\mark_1.jpg"]);
    assert_eq!(out.images.len(), 1);
    assert_eq!(codec.decodes.get(), 1);
}

/// A5: the new name would collide with another file in the folder, so a suffix is added.
#[test]
fn name_collision_gets_a_suffix() {
    let codec = FakeCodec::default();
    let mut b = bundle(
        "v07_image.hcg",
        &[("mark_1.png", fake("png", 4000, 3000, false, 3_000_000))],
    );
    let stamp = parse(&b.hcg).expect("parse").images()[0].stamp;
    let squatter = subfolder_file_name("C:\\test\\mark_1.jpg", stamp);
    b.images.push(BundleFile {
        name: squatter.clone(),
        bytes: b"other".to_vec(),
    });
    let policy = ResizePolicy::default();
    let result = apply(
        &codec,
        &b,
        analyze(&codec, &b, &policy),
        &policy,
        &mut NoProgress,
    )
    .expect("ok");
    assert_eq!(
        result.files[0].new_path.as_deref(),
        Some("C:\\test\\mark_1_1.jpg")
    );
    let out = result.output.expect("output");
    let squatter_file = out
        .images
        .iter()
        .find(|f| f.name == squatter)
        .expect("kept");
    assert_eq!(
        squatter_file.bytes, b"other",
        "the unreferenced file is copied unchanged"
    );
    assert_eq!(result.plan.unreferenced, [squatter]);
}

/// Alpha keeps PNG, so nothing is renamed; formats the tool does not touch stay as they are.
#[test]
fn alpha_png_and_gif_keep_their_names() {
    let codec = FakeCodec::default();
    let b = bundle(
        "v11_two_images.hcg",
        &[
            ("mark_1.png", fake("png", 4000, 3000, true, 3_000_000)),
            ("mark_2.png", fake("gif", 4000, 3000, false, 3_000_000)),
        ],
    );
    let policy = ResizePolicy::default();
    let result = apply(
        &codec,
        &b,
        analyze(&codec, &b, &policy),
        &policy,
        &mut NoProgress,
    )
    .expect("ok");
    assert_eq!(
        result.plan.files[1].action,
        PlannedAction::Keep(KeepReason::NotResized(ImageFormat::Gif))
    );
    assert_eq!(result.files.len(), 1);
    assert!(
        matches!(&result.files[0].outcome.action, ImageAction::Converted(c) if c.format == OutputFormat::Png)
    );
    assert_eq!(result.files[0].new_path, None);
    let out = result.output.expect("output");
    let before: Vec<_> = b.images.iter().map(|f| &f.name).collect();
    let after: Vec<_> = out.images.iter().map(|f| &f.name).collect();
    assert_eq!(after, before);
}

#[test]
fn missing_embedded_and_transitions_are_reported() {
    let codec = FakeCodec::default();
    let policy = ResizePolicy::default();
    let missing = analyze(&codec, &bundle("v07_image.hcg", &[]), &policy);
    assert_eq!(missing.missing.len(), 1);
    assert_eq!(path_of(&missing.missing[0].source), "C:\\test\\mark_1.png");
    assert!(missing.files.is_empty());

    let embedded = analyze(&codec, &bundle("i05_embedded_png.hcg", &[]), &policy);
    assert!(
        matches!(embedded.embedded[..], [ref e] if e.source == ImageSource::Embedded { len: 2537 })
    );
    assert!(embedded.missing.is_empty() && embedded.files.is_empty());

    let pages = analyze(&codec, &bundle("p3_MIX_2s.hcg", &[]), &policy);
    assert!(matches!(
        pages.transitions[..],
        [p] if p.index == 1 && p.transition == PageTransition::Effect { kind: TransitionKind::Mix, duration_ms: 2000 }
    ));
    let cut = analyze(&codec, &bundle("p1_cut_ref.hcg", &[]), &policy);
    assert!(cut.transitions.is_empty());
}

/// A9: an unreadable hcg fails only its own bundle; it gets a report-only output.
#[test]
fn broken_bundle_is_isolated() {
    let mut broken = sample_hcg("v00_empty.hcg");
    broken[8] = 0x77; // version 0x77
    let input = |path: &str, bytes: Vec<u8>| InputFile {
        path: path.into(),
        bytes,
        name_encoding: NameEncoding::Ascii,
    };
    let files = vec![
        input("old.hcg", broken),
        input("v12.hcg", sample_hcg("v12_sameimg_twice.hcg")),
    ];
    let result = run(
        files,
        &FakeCodec::default(),
        &ResizePolicy::default(),
        &mut NoProgress,
    )
    .expect("ok");
    let old = &result.bundles[0];
    assert!(matches!(old.failure, Some(ApplyFailure::Parse(_))) && old.output.is_none());
    assert!(result.bundles[1].output.is_some());
    let out = result.output_files(|b| format!("report for {}", b.plan.stem));
    let paths: Vec<_> = out.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "old_optimized/report.txt",
            "v12_optimized/v12.hcg",
            "v12_optimized/report.txt"
        ]
    );
}

struct CancelAfter {
    reports: usize,
    limit: usize,
}

impl Progress for CancelAfter {
    fn report(&mut self, _: usize, _: usize, _: &str) {
        self.reports += 1;
    }
    fn is_cancelled(&self) -> bool {
        self.reports >= self.limit
    }
}

/// A10: cancellation between images gives no output.
#[test]
fn cancellation_stops_between_images() {
    let codec = FakeCodec::default();
    let b = bundle(
        "speedtest.hcg",
        &[
            (
                "bg_6000x4000_photo.jpg",
                fake("jpg", 6000, 4000, false, 23_000_000),
            ),
            (
                "bg_1080_noise.png",
                fake("png", 1920, 1080, false, 6_000_000),
            ),
        ],
    );
    let policy = ResizePolicy::default();
    let mut progress = CancelAfter {
        reports: 0,
        limit: 1,
    };
    assert_eq!(
        apply(
            &codec,
            &b,
            analyze(&codec, &b, &policy),
            &policy,
            &mut progress
        ),
        Err(Cancelled)
    );
    assert_eq!(codec.decodes.get(), 1, "stopped before the second image");
}

/// plan_run decodes nothing, even across bundles.
#[test]
fn plan_run_is_cheap() {
    let codec = FakeCodec::default();
    let b = bundle(
        "v07_image.hcg",
        &[("mark_1.png", fake("png", 4000, 3000, false, 3_000_000))],
    );
    let mut files = vec![InputFile {
        path: "v07_image.hcg".into(),
        bytes: b.hcg.clone(),
        name_encoding: NameEncoding::Ascii,
    }];
    files.extend(b.images.iter().map(|f| InputFile {
        path: format!("v07_image/{}", f.name),
        bytes: f.bytes.clone(),
        name_encoding: NameEncoding::Ascii,
    }));
    let plan = plan_run(files, &codec, &ResizePolicy::default());
    assert_eq!(plan.shrink_count(), 1);
    assert_eq!(codec.decodes.get(), 0);
}
