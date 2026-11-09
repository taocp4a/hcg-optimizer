//! The browser session, natively (issue #17).

use std::path::Path;

use hcg_adapters::archive::{ZipLimits, read_zip, write_zip};
use hcg_app::package::OutputFile;
use hcg_app::ports::{NoProgress, Progress};
use hcg_wasm::session::{SessionCore, policy_from_json};

fn sample_zip(stem: &str) -> Vec<u8> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../project/samples");
    let mut files = vec![OutputFile {
        path: format!("{stem}.hcg"),
        bytes: std::fs::read(root.join(format!("{stem}.hcg"))).expect("hcg"),
    }];
    if let Ok(entries) = std::fs::read_dir(root.join(stem)) {
        for e in entries.filter_map(Result::ok) {
            files.push(OutputFile {
                path: format!("{stem}/{}", e.file_name().to_string_lossy()),
                bytes: std::fs::read(e.path()).expect("image"),
            });
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    write_zip(files).expect("zip")
}

#[derive(Default)]
struct Recorder(Vec<(usize, usize, String)>);

impl Progress for Recorder {
    fn report(&mut self, done: usize, total: usize, message: &str) {
        self.0.push((done, total, message.to_string()));
    }
}

#[test]
fn plan_then_optimize() {
    let mut session = SessionCore::new(sample_zip("speedtest"), "").expect("session");
    let plan: serde_json::Value = serde_json::from_str(session.plan_json()).expect("plan JSON");
    assert_eq!(plan["summary"]["files_to_shrink"], 2);
    assert_eq!(plan["summary"]["files_total"], 3);
    assert_eq!(plan["summary"]["nothing_to_do"], false);
    let headline = plan["summary"]["headline"].as_str().expect("headline");
    assert!(
        headline.starts_with("2 of 3 images will be shrunk"),
        "{headline}"
    );
    assert_eq!(plan["bundles"][0]["files"][0]["action"], "shrink");
    assert_eq!(plan["bundles"][0]["files"][0]["target"]["width"], 1620);
    assert_eq!(session.reports_json(), "[]");

    let mut progress = Recorder::default();
    let zip = session.optimize(&mut progress).expect("optimize");
    let named: Vec<_> = progress
        .0
        .iter()
        .filter(|(_, _, m)| !m.is_empty())
        .collect();
    assert_eq!(
        named.len(),
        2,
        "one progress call per image: {:?}",
        progress.0
    );
    assert!(named.iter().all(|(_, total, _)| *total == 2));

    let paths: Vec<_> = read_zip(&zip, &ZipLimits::default())
        .expect("zip")
        .into_iter()
        .map(|f| f.path)
        .collect();
    assert!(
        paths.contains(&"speedtest_optimized/speedtest.hcg".to_string()),
        "{paths:?}"
    );
    assert!(
        paths.contains(&"speedtest_optimized/report.txt".to_string()),
        "{paths:?}"
    );
    let reports: serde_json::Value = serde_json::from_str(session.reports_json()).expect("reports");
    assert_eq!(reports[0]["status"], "ok");
    assert_eq!(reports[0]["summary"]["images_shrunk"], 2);

    assert!(
        session.optimize(&mut NoProgress).is_err(),
        "only once per session"
    );
}

#[test]
fn settings_are_validated() {
    assert_eq!(policy_from_json("{}").expect("defaults").max_bytes, 1 << 20);
    let p = policy_from_json(r#"{"max_width":1280,"max_height":720,"max_bytes":500000,"jpeg_quality":80,"min_jpeg_quality":40}"#)
        .expect("valid");
    assert_eq!(
        (
            p.max_width,
            p.max_height,
            p.max_bytes,
            p.jpeg_quality,
            p.min_jpeg_quality
        ),
        (1280, 720, 500_000, 80, 40)
    );
    for bad in [
        r#"{"jpeg_quality":120}"#,
        r#"{"min_jpeg_quality":95}"#,
        r#"{"max_width":0}"#,
        r#"{"colour":1}"#,
        "not json",
    ] {
        let err = policy_from_json(bad).expect_err(bad);
        assert!(err.starts_with("Invalid settings"), "{bad}: {err}");
    }
    assert!(SessionCore::new(sample_zip("v07_image"), r#"{"jpeg_quality":0}"#).is_err());
}

#[test]
fn bad_input_gives_messages() {
    let err = SessionCore::new(b"not a zip".to_vec(), "")
        .err()
        .expect("error");
    assert!(
        err.starts_with("This file could not be opened as a zip"),
        "{err}"
    );

    let no_hcg = write_zip(vec![OutputFile {
        path: "notes.txt".into(),
        bytes: b"x".to_vec(),
    }])
    .expect("zip");
    let mut session = SessionCore::new(no_hcg, "").expect("readable zip");
    let plan: serde_json::Value = serde_json::from_str(session.plan_json()).expect("plan");
    assert_eq!(plan["summary"]["nothing_to_do"], true);
    assert_eq!(
        plan["summary"]["headline"],
        "No .hcg file found in the zip."
    );
    assert_eq!(plan["other_files"]["unpaired"][0], "notes.txt");
    assert!(
        session
            .optimize(&mut NoProgress)
            .expect_err("no hcg")
            .contains("No .hcg")
    );
}

/// Page overview (issue #19): from a zip or a bare .hcg, until optimize; settings validated.
#[test]
fn overview_pdf() {
    let mut session = SessionCore::new(sample_zip("layered"), "").expect("session");
    let pdf = session.overview_pdf(0, "").expect("pdf");
    assert!(pdf.starts_with(b"%PDF-"));
    assert!(session.overview_pdf(0, r#"{"per_sheet":20}"#).is_ok());
    assert!(session.overview_pdf(0, r#"{"per_sheet":9}"#).is_err());
    assert!(session.overview_pdf(0, r#"{"grid":"3x4"}"#).is_err());
    assert!(session.overview_pdf(1, "").is_err());

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../project/samples");
    let hcg = std::fs::read(root.join("speedtest.hcg")).expect("hcg");
    let mut bare = SessionCore::open(hcg, "speedtest.HCG", "").expect("bare hcg");
    let plan: serde_json::Value = serde_json::from_str(bare.plan_json()).expect("plan");
    assert_eq!(plan["bundles"].as_array().map(Vec::len), Some(1));
    assert!(
        !plan["bundles"][0]["missing"]
            .as_array()
            .expect("missing")
            .is_empty()
    );
    assert!(bare.overview_pdf(0, "").expect("pdf").starts_with(b"%PDF-"));
    bare.optimize(&mut NoProgress).expect("optimize");
    assert!(
        bare.overview_pdf(0, "").is_ok(),
        "still there after optimize"
    );

    // Never asked for before optimizing: prepared from the optimized hcg.
    let mut fresh = SessionCore::new(sample_zip("speedtest"), "").expect("session");
    fresh.optimize(&mut NoProgress).expect("optimize");
    let after = fresh
        .overview_pdf(0, r#"{"per_sheet":6}"#)
        .expect("pdf after optimize");
    assert!(after.starts_with(b"%PDF-"));

    let mut zip_named = SessionCore::open(sample_zip("layered"), "layered.zip", "").expect("zip");
    assert!(zip_named.overview_pdf(0, "").is_ok());
}

/// Issue #21: `"language":"ko"` gives Korean plan, reports and `점검결과.txt`.
#[test]
fn korean_session() {
    let mut session =
        SessionCore::new(sample_zip("speedtest"), r#"{"language":"ko"}"#).expect("session");
    let plan: serde_json::Value = serde_json::from_str(session.plan_json()).expect("plan");
    assert_eq!(plan["language"], "ko");
    let headline = plan["summary"]["headline"].as_str().expect("headline");
    assert!(headline.starts_with("축소 대상 이미지"), "{headline}");
    let zip = session.optimize(&mut NoProgress).expect("optimize");
    let names: Vec<String> = read_zip(&zip, &ZipLimits::default())
        .expect("zip")
        .into_iter()
        .map(|f| f.path)
        .collect();
    assert!(
        names.contains(&"speedtest_optimized/점검결과.txt".to_string()),
        "{names:?}"
    );
    let reports: serde_json::Value = serde_json::from_str(session.reports_json()).expect("json");
    assert!(
        reports[0]["text"]
            .as_str()
            .expect("text")
            .starts_with("hcg 이미지 최적화 점검 결과")
    );
    assert!(SessionCore::new(sample_zip("speedtest"), r#"{"language":"fr"}"#).is_err());
}
