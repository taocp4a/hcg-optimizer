//! Runs the built `hcg-optimizer` binary (issue #15, contracts L1–L7).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use hcg_adapters::archive::{ZipLimits, read_zip, write_zip};
use hcg_app::package::OutputFile;

fn samples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../project/samples")
}

/// A fresh, empty directory for one test.
fn tmp(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("cli")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    dir
}

fn cli(args: &[&dyn AsRef<std::ffi::OsStr>]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hcg-optimizer"))
        .args(args.iter().map(|a| a.as_ref()))
        .output()
        .expect("run hcg-optimizer")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// A zip of `<stem>.hcg` + `<stem>/` from the samples, written into `dir`.
fn sample_zip(dir: &Path, stem: &str) -> PathBuf {
    let mut files = vec![OutputFile {
        path: format!("{stem}.hcg"),
        bytes: std::fs::read(samples().join(format!("{stem}.hcg"))).expect("hcg"),
    }];
    if let Ok(entries) = std::fs::read_dir(samples().join(stem)) {
        for e in entries.filter_map(Result::ok) {
            files.push(OutputFile {
                path: format!("{stem}/{}", e.file_name().to_string_lossy()),
                bytes: std::fs::read(e.path()).expect("image"),
            });
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let path = dir.join(format!("{stem}.zip"));
    std::fs::write(&path, write_zip(files).expect("zip")).expect("write zip");
    path
}

fn zip_paths(path: &Path) -> Vec<String> {
    let bytes = std::fs::read(path).expect("output zip");
    read_zip(&bytes, &ZipLimits::default())
        .expect("valid zip")
        .into_iter()
        .map(|f| f.path)
        .collect()
}

/// .hcg input with an explicit output: the folder is picked up, the zip and report are right.
#[test]
fn hcg_input_end_to_end() {
    let dir = tmp("hcg_input");
    let out = dir.join("out.zip");
    let o = cli(&[&samples().join("speedtest.hcg"), &"-o", &out]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert!(
        stdout(&o).contains("2 of 3 images shrunk"),
        "{}",
        stdout(&o)
    );
    assert!(
        stderr(&o).contains("[1/2]") && stderr(&o).contains("Wrote"),
        "{}",
        stderr(&o)
    );
    let paths = zip_paths(&out);
    for expected in [
        "speedtest_optimized/speedtest.hcg",
        "speedtest_optimized/speedtest/3F8B560Bbg_1080_noise.jpg",
        "speedtest_optimized/report.txt",
    ] {
        assert!(
            paths.iter().any(|p| p == expected),
            "{expected} missing: {paths:?}"
        );
    }
    assert!(
        !dir.join(".out.zip.partial").exists(),
        "L2: no temporary file left"
    );
}

/// Zip input with the default output name; L5: a second run gives identical bytes.
#[test]
fn zip_input_default_output_and_determinism() {
    let dir = tmp("zip_input");
    let input = sample_zip(&dir, "v11_two_images");
    let o = cli(&[&input]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let out = dir.join("v11_two_images_optimized.zip");
    let first = std::fs::read(&out).expect("default output");
    let o = cli(&[&input, &"--force"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(
        std::fs::read(&out).expect("output"),
        first,
        "L5: deterministic"
    );
}

/// L4: --dry-run prints the plan and writes nothing.
#[test]
fn dry_run_writes_nothing() {
    let dir = tmp("dry_run");
    let input = sample_zip(&dir, "speedtest");
    let o = cli(&[&input, &"--dry-run"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let text = stdout(&o);
    assert!(
        text.contains("2 of 3 referenced file(s) will be shrunk"),
        "{text}"
    );
    assert!(text.contains("shrink to fit 1620x1080"), "{text}");
    let entries: Vec<_> = std::fs::read_dir(&dir)
        .expect("dir")
        .filter_map(Result::ok)
        .collect();
    assert_eq!(entries.len(), 1, "only the input exists");
}

/// L1: existing output and output == input are refused; --force overwrites.
#[test]
fn overwrite_protection() {
    let dir = tmp("overwrite");
    let input = sample_zip(&dir, "v07_image");
    let out = dir.join("taken.zip");
    std::fs::write(&out, b"keep me").expect("existing");
    let o = cli(&[&input, &"-o", &out]);
    assert_eq!(o.status.code(), Some(2));
    assert!(stderr(&o).contains("already exists"), "{}", stderr(&o));
    assert_eq!(std::fs::read(&out).expect("still there"), b"keep me");

    let o = cli(&[&input, &"-o", &input, &"--force"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(stderr(&o).contains("overwrite the input"), "{}", stderr(&o));

    let o = cli(&[&input, &"-o", &out, &"--force"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert!(
        zip_paths(&out)
            .iter()
            .any(|p| p == "v07_image_optimized/report.txt")
    );
}

/// L6/L7: exit codes for bad settings, bad input, no hcg, and a failed bundle.
#[test]
fn exit_codes() {
    let dir = tmp("exit_codes");
    let input = sample_zip(&dir, "v07_image");
    for bad in [
        &["--quality", "120"][..],
        &["--max-size", "big"],
        &["--max-bytes", "1 GB?"],
        &["--min-quality", "95"],
    ] {
        let mut args: Vec<&dyn AsRef<std::ffi::OsStr>> = vec![&input];
        args.extend(bad.iter().map(|a| a as &dyn AsRef<std::ffi::OsStr>));
        let o = cli(&args);
        assert_eq!(o.status.code(), Some(2), "{bad:?}: {}", stderr(&o));
    }
    assert_eq!(cli(&[&dir.join("missing.zip")]).status.code(), Some(3));
    let txt = dir.join("notes.txt");
    std::fs::write(&txt, b"x").expect("txt");
    assert_eq!(cli(&[&txt]).status.code(), Some(2), "not a zip or hcg");

    let empty = dir.join("empty.zip");
    std::fs::write(
        &empty,
        write_zip(vec![OutputFile {
            path: "a.txt".into(),
            bytes: vec![1],
        }])
        .expect("zip"),
    )
    .expect("write");
    let o = cli(&[&empty, &"--dry-run"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("no .hcg file"), "{}", stderr(&o));

    let mut old = std::fs::read(samples().join("v00_empty.hcg")).expect("hcg");
    old[8] = 0x77;
    let old_path = dir.join("old.hcg");
    std::fs::write(&old_path, old).expect("write");
    let o = cli(&[&old_path]);
    assert_eq!(o.status.code(), Some(1), "a failed bundle");
    assert!(stdout(&o).contains("What to do:"), "{}", stdout(&o));
    assert_eq!(
        zip_paths(&dir.join("old_optimized.zip")),
        ["old_optimized/report.txt"]
    );
}

/// --json prints the structured reports; policy flags reach the core.
#[test]
fn json_output_and_policy_flags() {
    let dir = tmp("json");
    let input = sample_zip(&dir, "v10_bg");
    let o = cli(&[
        &input,
        &"--json",
        &"--max-size",
        &"640x360",
        &"--max-bytes",
        &"200KB",
        &"--quality",
        &"80",
    ]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let reports: serde_json::Value = serde_json::from_slice(&o.stdout).expect("JSON");
    let report = &reports[0];
    assert_eq!(report["schema"], 1);
    assert_eq!(report["policy"]["max_width"], 640);
    assert_eq!(report["policy"]["max_bytes"], 200 * 1024);
    assert_eq!(report["policy"]["jpeg_quality"], 80);
    let image = &report["images"][0];
    assert_eq!(image["action"], "shrunk");
    assert_eq!(
        (
            image["after"]["width"].as_u64(),
            image["after"]["height"].as_u64()
        ),
        (Some(640), Some(360))
    );
}
