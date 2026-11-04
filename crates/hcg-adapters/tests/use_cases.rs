//! The whole pipeline with the real codec over `project/samples` (issue #9).

use std::path::{Path, PathBuf};

use hcg_adapters::archive::{ZipLimits, read_zip, write_zip};
use hcg_adapters::image_codec::PureRustCodec;
use hcg_app::package::{InputFile, NameEncoding, OutputFile};
use hcg_app::policy::{ImageAction, ResizePolicy};
use hcg_app::ports::NoProgress;
use hcg_app::run::{RunResult, plan_run, run};
use hcg_format::{ImageSource, parse};
use image::GenericImageView;

fn samples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../project/samples")
}

/// Every `.hcg` and image-folder file under `project/samples` (or only the given stems).
fn sample_inputs(only: Option<&[&str]>) -> Vec<InputFile> {
    let root = samples_dir();
    let wanted = |stem: &str| only.is_none_or(|o| o.contains(&stem));
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&root).expect("samples") {
        let path = entry.expect("entry").path();
        let name = path
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned();
        if path.is_dir() {
            if !wanted(&name) {
                continue;
            }
            for image in std::fs::read_dir(&path).expect("folder") {
                let image = image.expect("entry").path();
                let file = image
                    .file_name()
                    .expect("name")
                    .to_string_lossy()
                    .into_owned();
                files.push(input(&format!("{name}/{file}"), &image));
            }
        } else if name.ends_with(".hcg") && wanted(name.trim_end_matches(".hcg")) {
            files.push(input(&name, &path));
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

fn input(path: &str, file: &Path) -> InputFile {
    InputFile {
        path: path.into(),
        bytes: std::fs::read(file).expect("read"),
        name_encoding: NameEncoding::Ascii,
    }
}

fn process(files: Vec<InputFile>) -> RunResult {
    run(
        files,
        &PureRustCodec::default(),
        &ResizePolicy::default(),
        &mut NoProgress,
    )
    .expect("not cancelled")
}

fn as_inputs(out: &[OutputFile]) -> Vec<InputFile> {
    out.iter()
        .map(|f| InputFile {
            path: f.path.clone(),
            bytes: f.bytes.clone(),
            name_encoding: NameEncoding::Ascii,
        })
        .collect()
}

/// End to end on the two realistic files, through a zip, then a second run over the output.
#[test]
fn layered_and_speedtest_end_to_end_and_idempotent() {
    let zip = write_zip(
        &sample_inputs(Some(&["layered", "speedtest"]))
            .into_iter()
            .map(|f| OutputFile {
                path: f.path,
                bytes: f.bytes,
            })
            .collect::<Vec<_>>(),
    )
    .expect("zip");
    let files = read_zip(&zip, &ZipLimits::default()).expect("read zip");
    let plan = plan_run(
        files.clone(),
        &PureRustCodec::default(),
        &ResizePolicy::default(),
    );
    // layered: big_0..2 + 6000x4000 photo; speedtest: photo + noise PNG.
    assert_eq!(plan.shrink_count(), 6);

    let result = process(files);
    for bundle in &result.bundles {
        assert_eq!(bundle.failure, None, "{}", bundle.plan.stem);
        let out = bundle.output.as_ref().expect("output");
        let parsed = parse(&out.hcg).expect("output hcg parses");
        for image in parsed.images() {
            let name = image.folder_file_name.expect("path image");
            let file = out
                .images
                .iter()
                .find(|f| f.name == name)
                .expect("A2: resolves");
            let (w, h) = image::load_from_memory(&file.bytes)
                .expect("decodes")
                .dimensions();
            assert!(w <= 1920 && h <= 1080, "{name}: {w}x{h}");
        }
    }
    let speedtest = result
        .bundles
        .iter()
        .find(|b| b.plan.stem == "speedtest")
        .expect("speedtest");
    let noise = speedtest
        .files
        .iter()
        .find(|f| f.name.ends_with("noise.png"))
        .expect("noise");
    assert_eq!(
        noise.new_path.as_deref(),
        Some("C:\\test\\bg_1080_noise.jpg")
    );
    let parsed = parse(&speedtest.output.as_ref().expect("out").hcg).expect("parse");
    assert!(
        parsed
            .images()
            .iter()
            .any(|i| i.source == ImageSource::Path("C:\\test\\bg_1080_noise.jpg".into()))
    );

    // A7: feeding the output back in changes nothing. The second run's output sits under
    // `<stem>_optimized_optimized/`, so compare paths below the root folder.
    let first = result.output_files(|_| String::new());
    let second = process(as_inputs(&first));
    for bundle in &second.bundles {
        assert!(
            bundle
                .files
                .iter()
                .all(|f| matches!(f.outcome.action, ImageAction::Unchanged(_))),
            "{}: {:?}",
            bundle.plan.stem,
            bundle.files
        );
    }
    let below_root = |out: &[OutputFile]| -> Vec<(String, Vec<u8>)> {
        out.iter()
            .map(|f| {
                (
                    f.path.split_once('/').expect("root").1.to_string(),
                    f.bytes.clone(),
                )
            })
            .collect()
    };
    assert_eq!(
        below_root(&second.output_files(|_| String::new())),
        below_root(&first),
        "second run must reproduce the first output"
    );
}

/// Every sample in one input: all bundles succeed, references stay intact, and property-test
/// files without folders report their images as missing.
#[test]
fn all_samples_in_one_run() {
    let result = process(sample_inputs(None));
    assert_eq!(result.bundles.len(), 91);
    assert!(result.unpaired.is_empty(), "{:?}", result.unpaired);
    for bundle in &result.bundles {
        assert_eq!(bundle.failure, None, "{}", bundle.plan.stem);
        let out = bundle.output.as_ref().expect("output");
        let parsed = parse(&out.hcg).expect("parse");
        let found = bundle
            .plan
            .files
            .iter()
            .map(|f| f.refs.len())
            .sum::<usize>();
        let resolving = parsed
            .images()
            .iter()
            .filter(|i| {
                i.folder_file_name
                    .as_ref()
                    .is_some_and(|n| out.images.iter().any(|f| &f.name == n))
            })
            .count();
        assert_eq!(resolving, found, "{}", bundle.plan.stem);
    }
    let get = |stem: &str| {
        result
            .bundles
            .iter()
            .find(|b| b.plan.stem == stem)
            .expect(stem)
    };
    assert_eq!(get("i04_sequence3").plan.missing.len(), 4);
    assert_eq!(get("i06_embedded_jpg").plan.embedded.len(), 1);
    assert_eq!(get("p2_L2R_2s").plan.transitions.len(), 1);
    // Unchanged bundles keep their hcg byte for byte.
    let v00 = get("v00_empty");
    assert_eq!(
        v00.output.as_ref().expect("out").hcg,
        std::fs::read(samples_dir().join("v00_empty.hcg")).expect("read")
    );
}

/// The report of a real run: status, counts, the rename, the over-limit warning; report.txt in
/// the zip is its text.
#[test]
fn speedtest_report() {
    let policy = ResizePolicy::default();
    let result = process(sample_inputs(Some(&["speedtest"])));
    let report = &result.reports(&policy)[0];
    let json: serde_json::Value =
        serde_json::from_str(&hcg_app::report::to_json(report)).expect("valid JSON");
    assert_eq!(json["status"], "ok");
    assert_eq!(json["summary"]["images_total"], 3);
    assert_eq!(json["summary"]["images_shrunk"], 2);
    assert_eq!(json["summary"]["images_renamed"], 1);
    let codes: Vec<&str> = json["warnings"]
        .as_array()
        .expect("warnings")
        .iter()
        .filter_map(|w| w["code"].as_str())
        .collect();
    assert_eq!(
        codes,
        ["over_size_limit"],
        "the noise image stays over 1 MB"
    );
    let text = &report.text;
    assert!(
        text.contains("renamed to") && text.contains("C:\\test\\bg_1080_noise.jpg"),
        "{text}"
    );

    let out = result.package(&policy);
    let file = out
        .iter()
        .find(|f| f.path == "speedtest_optimized/report.txt")
        .expect("report.txt");
    assert!(
        file.bytes.starts_with(&[0xEF, 0xBB, 0xBF]) && file.bytes.windows(2).any(|w| w == b"\r\n")
    );
    eprintln!("{text}");
}
