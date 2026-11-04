//! Inspection report (issue #11, contracts R1–R7). Snapshots live in `tests/snapshots/`;
//! regenerate with `UPDATE_SNAPSHOTS=1 cargo test -p hcg-app --test report`.

mod common;

use std::path::PathBuf;

use common::{FakeCodec, bundle, fake, sample_hcg};
use hcg_app::analyze::{FilePlan, analyze};
use hcg_app::apply::{BundleResult, apply};
use hcg_app::package::{InputFile, NameEncoding};
use hcg_app::policy::{KeepReason, PlannedAction, ResizePolicy};
use hcg_app::ports::{ImageFormat, ImageProbe, NoProgress};
use hcg_app::report::{RunNotes, Status, build_report, to_json};
use hcg_app::run::run;
use hcg_format::parse;

fn check_snapshot(name: &str, actual: &str) {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests", "snapshots", name]
        .iter()
        .collect();
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::create_dir_all(path.parent().expect("dir")).expect("mkdir");
        std::fs::write(&path, actual).expect("write snapshot");
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("{} missing; run with UPDATE_SNAPSHOTS=1", path.display()));
    assert!(
        expected == actual,
        "{name} differs from the snapshot; run with UPDATE_SNAPSHOTS=1 and review the diff.\n{actual}"
    );
}

fn policy() -> ResizePolicy {
    // A small size limit so the fake noise image ends up over it (fake JPEG = w*h*q/1000 bytes).
    ResizePolicy {
        max_bytes: 100_000,
        ..ResizePolicy::default()
    }
}

/// A result containing every kind of report item.
fn every_kind() -> (BundleResult, RunNotes) {
    let codec = FakeCodec::default();
    let policy = policy();
    let speedtest = bundle(
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
    let mut plan = analyze(&codec, &speedtest, &policy);

    // A WebP and a GIF referenced by v11's two objects.
    let v11_refs = parse(&sample_hcg("v11_two_images.hcg"))
        .expect("parse")
        .images();
    for (image, format, w, h, bytes, reason) in [
        (
            &v11_refs[0],
            ImageFormat::WebP,
            800,
            600,
            50_000,
            KeepReason::NotSupportedByApp(ImageFormat::WebP),
        ),
        (
            &v11_refs[1],
            ImageFormat::Gif,
            320,
            240,
            9_000,
            KeepReason::NotResized(ImageFormat::Gif),
        ),
    ] {
        plan.files.push(FilePlan {
            name: image.folder_file_name.clone().expect("path image"),
            refs: vec![image.clone()],
            probe: Some(ImageProbe {
                format,
                dimensions: Some((w, h)),
            }),
            bytes,
            action: PlannedAction::Keep(reason),
        });
    }
    let others = |name: &str| analyze(&codec, &bundle(name, &[]), &policy);
    plan.missing.extend(others("v07_image.hcg").missing);
    plan.embedded
        .extend(others("i05_embedded_png.hcg").embedded);
    plan.embedded.extend(others("i05_resaved.hcg").embedded);
    plan.transitions.extend(others("p2_L2R_2s.hcg").transitions);
    plan.unreferenced.push("0000ABCDextra.png".into());

    let result = apply(&codec, &speedtest, plan, &policy, &mut NoProgress).expect("not cancelled");
    let notes = RunNotes {
        unpaired: vec!["readme.txt".into()],
        ignored: 2,
    };
    (result, notes)
}

/// R1, R3, R6: every item kind appears; the JSON and its text rendering match the snapshots.
#[test]
fn report_with_every_item_kind() {
    let (result, notes) = every_kind();
    let report = build_report(&result, &notes, &policy());
    assert_eq!(report.status, Status::Ok);
    assert_eq!(report.images.len(), 5);
    assert_eq!(
        (report.summary.images_shrunk, report.summary.images_renamed),
        (2, 1)
    );
    assert_eq!(
        (
            report.missing.len(),
            report.embedded.len(),
            report.transitions.len()
        ),
        (1, 2, 1)
    );
    assert_eq!(report.unreferenced_files, 1);
    let codes: Vec<&str> = report.warnings.iter().map(|w| w.code.as_str()).collect();
    for code in [
        "not_supported_by_app",
        "over_size_limit",
        "missing_image",
        "transition_not_cut",
    ] {
        assert!(codes.contains(&code), "{code} missing from {codes:?}");
    }
    assert_eq!(report.summary.warnings, report.warnings.len());
    assert!(
        report
            .text
            .starts_with("hcg-optimizer report: speedtest.hcg\n")
    );
    assert!(report.text.contains(&report.summary.headline));

    check_snapshot("every_kind.json", &to_json(&report));
    check_snapshot("every_kind.txt", &report.text);
}

/// R5: an unsupported version gives a failed report with advice.
#[test]
fn failed_bundle_report() {
    let mut hcg = sample_hcg("v00_empty.hcg");
    hcg[8] = 0x77;
    let mut broken = bundle("v00_empty.hcg", &[]);
    broken.hcg = hcg;
    let codec = FakeCodec::default();
    let policy = ResizePolicy::default();
    let result = apply(
        &codec,
        &broken,
        analyze(&codec, &broken, &policy),
        &policy,
        &mut NoProgress,
    )
    .expect("ok");
    let report = build_report(&result, &RunNotes::default(), &policy);
    assert_eq!(report.status, Status::Failed);
    let failure = report.failure.as_ref().expect("failure");
    assert_eq!(failure.code, "unsupported_version");
    assert!(failure.advice.contains("save it again"));
    assert!(report.hcg.is_none());
    check_snapshot("failed.json", &to_json(&report));
    check_snapshot("failed.txt", &report.text);
}

/// R2: the same result gives byte-identical JSON.
#[test]
fn report_is_deterministic() {
    let (a, notes) = every_kind();
    let (b, _) = every_kind();
    assert_eq!(
        to_json(&build_report(&a, &notes, &policy())),
        to_json(&build_report(&b, &notes, &policy()))
    );
}

/// The zip gets each report rendered as report.txt (UTF-8 BOM + CRLF), including failed
/// bundles; viewers get the same reports as structured data.
#[test]
fn package_writes_rendered_report_txt() {
    let mut broken = sample_hcg("v00_empty.hcg");
    broken[8] = 0x77;
    let input = |path: &str, bytes: Vec<u8>| InputFile {
        path: path.into(),
        bytes,
        name_encoding: NameEncoding::Ascii,
    };
    let files = vec![
        input("old.hcg", broken),
        input("v12.hcg", sample_hcg("v12_sameimg_twice.hcg")),
        input("notes.txt", b"hello".to_vec()),
    ];
    let policy = ResizePolicy::default();
    let result = run(files, &FakeCodec::default(), &policy, &mut NoProgress).expect("ok");

    let reports = result.reports(&policy);
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].status, Status::Failed);
    assert_eq!(reports[1].status, Status::Ok);
    assert_eq!(reports[1].other_files.unpaired, ["notes.txt"]);
    // v12 has no image folder here: its two references are one missing file.
    assert_eq!(reports[1].missing.len(), 1);
    let json: serde_json::Value = serde_json::from_str(&to_json(&reports[1])).expect("valid JSON");
    assert_eq!(json["schema"], 1);

    let out = result.package(&policy);
    let texts: Vec<_> = out
        .iter()
        .filter(|f| f.path.ends_with("/report.txt"))
        .collect();
    let paths: Vec<_> = texts.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        paths,
        ["old_optimized/report.txt", "v12_optimized/report.txt"]
    );
    assert!(
        !out.iter().any(|f| f.path.ends_with(".json")),
        "no JSON in the zip"
    );
    for (file, report) in texts.iter().zip(&reports) {
        let expected = format!("\u{feff}{}", report.text.replace('\n', "\r\n"));
        assert_eq!(
            String::from_utf8(file.bytes.clone()).expect("UTF-8"),
            expected
        );
    }
}
