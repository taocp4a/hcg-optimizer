//! Contract tests for hcg-format over every file in `project/samples` (see issue #3).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use hcg_format::{
    EditError, HcgFile, ImageRole, ImageSource, PageTransition, ParseErrorKind, TransitionKind,
    parse,
};
use serde_json::{Value, json};

fn samples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../project/samples")
}

/// All sample `.hcg` files as (path relative to samples/ with `/`, bytes), sorted.
fn samples() -> Vec<(String, Vec<u8>)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("read samples dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "hcg") {
                out.push(path);
            }
        }
    }
    let root = samples_dir();
    let mut paths = Vec::new();
    walk(&root, &mut paths);
    let mut out: Vec<_> = paths
        .into_iter()
        .map(|p| {
            let rel = p
                .strip_prefix(&root)
                .expect("under samples")
                .to_string_lossy()
                .replace('\\', "/");
            (rel, std::fs::read(&p).expect("read sample"))
        })
        .collect();
    out.sort();
    assert!(
        out.len() >= 20,
        "expected at least 20 samples, found {}",
        out.len()
    );
    out
}

fn parse_ok(name: &str, bytes: &[u8]) -> HcgFile {
    parse(bytes).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// Probe offsets for truncation/mutation tests: every offset in the first `dense` bytes (file
/// header, layers, and objects live there), about `sparse` evenly spaced offsets after that
/// (mostly raw thumbnail data), and the last 16 bytes.
fn probe_offsets(len: usize, dense: usize, sparse: usize) -> Vec<usize> {
    let dense = len.min(dense);
    let mut v: Vec<usize> = (0..dense).collect();
    v.extend((dense..len).step_by((len / sparse).max(1)));
    v.extend(len.saturating_sub(16)..len);
    v.sort_unstable();
    v.dedup();
    v
}

/// C1: parse -> to_bytes is byte-identical for every sample.
#[test]
fn c1_roundtrip_is_byte_identical() {
    for (name, bytes) in samples() {
        let file = parse_ok(&name, &bytes);
        assert!(file.to_bytes() == bytes, "{name}: roundtrip differs");
        assert_eq!(file.version(), 0x78, "{name}");
    }
}

/// C3/C4: every truncated prefix is rejected with an error, never a panic.
#[test]
fn c3_truncated_input_is_an_error() {
    for (name, bytes) in samples() {
        for cut in probe_offsets(bytes.len(), 4096, 500) {
            assert!(
                parse(&bytes[..cut]).is_err(),
                "{name}: prefix of {cut} bytes was accepted"
            );
        }
    }
}

/// C3/C4: corrupted bytes never panic, and any input that still parses roundtrips exactly.
#[test]
fn c3_mutated_input_never_panics() {
    for (name, bytes) in samples() {
        let mut buf = bytes.clone();
        // Also cover the object records around every SafeBMP (objects on later pages sit after
        // earlier pages' thumbnails, beyond the dense prefix).
        let mut offsets = probe_offsets(bytes.len(), 1024, 100);
        for (i, _) in bytes
            .windows(8)
            .enumerate()
            .filter(|(_, w)| w == b"SafeBMP ")
        {
            offsets.extend(i.saturating_sub(0x200)..(i + 0x40).min(bytes.len()));
        }
        offsets.sort_unstable();
        offsets.dedup();
        for at in offsets {
            for value in [0x00, 0xFF, 0x7F, 0x80] {
                let original = buf[at];
                buf[at] = value;
                if let Ok(file) = parse(&buf) {
                    assert!(
                        file.to_bytes() == buf,
                        "{name}: mutation at {at:#x} parsed but did not roundtrip"
                    );
                }
                buf[at] = original;
            }
        }
    }
}

#[test]
fn c3_trailing_bytes_are_an_error() {
    let (name, mut bytes) = samples().remove(0);
    let len = bytes.len();
    bytes.push(0);
    let err = parse(&bytes).expect_err(&name);
    assert_eq!(err.kind, ParseErrorKind::TrailingBytes(1));
    assert_eq!(err.offset, len);
}

#[test]
fn unsupported_version_is_rejected() {
    let (_, mut bytes) = samples().remove(0);
    bytes[8..12].copy_from_slice(&0x77i32.to_le_bytes());
    let err = parse(&bytes).expect_err("version 0x77");
    assert_eq!(err.kind, ParseErrorKind::UnsupportedVersion(0x77));
    assert_eq!(err.offset, 8);
    assert_eq!(
        err.to_string(),
        "file header: unsupported file version 0x77 (only 0x78 is supported) at 0x8"
    );
}

#[test]
fn bad_magic_is_rejected() {
    let err = parse(b"NOTANHCGFILE").expect_err("short garbage");
    assert!(
        matches!(err.kind, ParseErrorKind::UnexpectedEof { .. }),
        "{err}"
    );
    let (_, mut bytes) = samples().remove(0);
    bytes[0] = b'X';
    assert!(matches!(
        parse(&bytes).expect_err("magic").kind,
        ParseErrorKind::BadMagic { .. }
    ));
}

/// Builds the same summary as project/scripts/reference_summary.py from the public API.
fn summarize(file: &HcgFile) -> Value {
    let images: Vec<Value> = file
        .images()
        .into_iter()
        .map(|img| {
            let role = match img.role {
                ImageRole::Primary => "primary".to_string(),
                ImageRole::Sequence(i) => format!("sequence:{i}"),
            };
            let mut v = json!({
                "page": img.page, "layer": img.layer, "object": img.object,
                "role": role, "stamp": img.stamp,
            });
            let map = v.as_object_mut().expect("object");
            match img.source {
                ImageSource::Path(p) => {
                    map.insert("path".into(), json!(p));
                    map.insert("folder_file_name".into(), json!(img.folder_file_name));
                }
                ImageSource::Embedded { len } => {
                    map.insert("embedded_len".into(), json!(len));
                }
                ImageSource::Empty => {
                    map.insert("empty".into(), json!(true));
                }
            }
            v
        })
        .collect();
    let pages: Vec<Value> = file
        .pages()
        .map(|p| {
            let (enabled, kind, ms) = match p.transition {
                PageTransition::Global => (0, None, None),
                PageTransition::Effect { kind, duration_ms } => {
                    let k = match kind {
                        TransitionKind::Cut => 0,
                        TransitionKind::Mix => 1,
                        TransitionKind::LeftToRight => 2,
                        TransitionKind::RightToLeft => 3,
                        TransitionKind::BottomToTop => 4,
                        TransitionKind::TopToBottom => 5,
                        TransitionKind::Unknown(n) => n,
                    };
                    (1, Some(k), Some(duration_ms))
                }
            };
            json!({
                "width": p.width, "height": p.height,
                "transition_enabled": enabled, "transition_kind": kind, "transition_ms": ms,
                "object_count": p.object_count,
            })
        })
        .collect();
    json!({
        "version": file.version(), "global_objects": file.global_object_count(),
        "images": images, "pages": pages,
    })
}

/// C6: results match the Python reference (project/samples/reference_summary.json).
#[test]
fn c6_matches_python_reference() {
    let text = std::fs::read_to_string(samples_dir().join("reference_summary.json"))
        .expect("reference_summary.json (run project/scripts/reference_summary.py)");
    let reference: BTreeMap<String, Value> = serde_json::from_str(&text).expect("valid JSON");
    let files = samples();
    assert_eq!(
        reference.len(),
        files.len(),
        "reference covers a different set of samples"
    );
    for (name, bytes) in files {
        let file = parse_ok(&name, &bytes);
        let mut expected = reference
            .get(&name)
            .unwrap_or_else(|| panic!("{name} missing"))
            .clone();
        // The reference stores transition kind/time even when disabled; the Rust API reports
        // only "uses the global effect" in that case, so compare those fields only when enabled.
        for page in expected["pages"].as_array_mut().expect("pages") {
            if page["transition_enabled"] == json!(0) {
                page["transition_kind"] = Value::Null;
                page["transition_ms"] = Value::Null;
            } else {
                page["transition_enabled"] = json!(1);
            }
        }
        assert_eq!(summarize(&file), expected, "{name}");
    }
}

/// C5: an edit changes only that SafeBMP's size field and payload.
#[test]
fn c5_edit_changes_only_size_and_payload() {
    let mut edited_any = false;
    for (name, bytes) in samples() {
        let file = parse_ok(&name, &bytes);
        let images = file.images();
        let magic_offsets: Vec<usize> = bytes
            .windows(8)
            .enumerate()
            .filter(|(_, w)| w == b"SafeBMP ")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(magic_offsets.len(), images.len(), "{name}: SafeBMP count");

        for (img, &offset) in images.iter().zip(&magic_offsets) {
            let ImageSource::Path(old) = &img.source else {
                continue;
            };
            let new_path = format!("{old}.resized.jpg");
            let mut edited = file.clone();
            edited.set_image_path(img.slot, &new_path).expect("edit");
            let out = edited.to_bytes();

            let old_len = old.encode_utf16().count() * 2;
            let new_payload: Vec<u8> = new_path.encode_utf16().flat_map(u16::to_le_bytes).collect();
            let mut expected = bytes[..offset + 8].to_vec();
            expected.extend_from_slice(&(new_payload.len() as i64).to_le_bytes());
            expected.extend_from_slice(&bytes[offset + 16..offset + 32]);
            expected.extend_from_slice(&new_payload);
            expected.extend_from_slice(&bytes[offset + 32 + old_len..]);
            assert!(
                out == expected,
                "{name}: edit at {offset:#x} changed other bytes"
            );

            // Re-parse: only this image's path (and folder name) differ.
            let reparsed = parse_ok(&name, &out);
            assert!(reparsed.pages().eq(file.pages()), "{name}: pages changed");
            for (before, after) in images.iter().zip(reparsed.images()) {
                if before.slot == img.slot {
                    assert_eq!(after.source, ImageSource::Path(new_path.clone()));
                    assert_eq!(after.stamp, before.stamp);
                    assert_ne!(after.folder_file_name, before.folder_file_name);
                } else {
                    assert_eq!(&after, before, "{name}: another image changed");
                }
            }
            edited_any = true;
        }
    }
    assert!(edited_any, "no path images found in samples");
}

#[test]
fn edit_rejects_bad_input() {
    let (name, bytes) = samples()
        .into_iter()
        .find(|(n, _)| n.ends_with("v07_image.hcg"))
        .expect("v07");
    let mut file = parse_ok(&name, &bytes);
    let slot = file.images()[0].slot;
    assert_eq!(file.set_image_path(slot, ""), Err(EditError::EmptyPath));
    let long = "a".repeat(hcg_format::MAX_PATH_UNITS + 1);
    assert_eq!(
        file.set_image_path(slot, &long),
        Err(EditError::PathTooLong(long.len()))
    );
    // A slot from a file with more pages does not exist here.
    let (n2, b2) = samples()
        .into_iter()
        .find(|(n, _)| n.ends_with("speedtest.hcg"))
        .expect("speedtest");
    let other = parse_ok(&n2, &b2);
    let foreign = other
        .images()
        .into_iter()
        .find(|i| i.page.is_some_and(|p| p > 0))
        .expect("page > 0");
    assert_eq!(
        file.set_image_path(foreign.slot, "C:\\x.jpg"),
        Err(EditError::InvalidSlot)
    );
    assert!(
        file.to_bytes() == bytes,
        "failed edits must not change the file"
    );
}

/// C7: every path image of a sample with an image folder resolves to a real file in it.
#[test]
fn c7_folder_file_names_exist() {
    let mut checked = 0;
    for (name, bytes) in samples() {
        let folder = samples_dir().join(name.trim_end_matches(".hcg"));
        if !folder.is_dir() {
            continue;
        }
        for img in parse_ok(&name, &bytes).images() {
            let file_name = img.folder_file_name.expect("path image");
            assert!(
                folder.join(&file_name).is_file(),
                "{name}: {file_name} not in folder"
            );
            checked += 1;
        }
    }
    assert!(checked >= 11, "only {checked} images checked");
}

#[test]
fn page_transitions_are_read() {
    let (name, bytes) = samples()
        .into_iter()
        .find(|(n, _)| n.ends_with("p01_page2_effect_L2R_2s.hcg"))
        .expect("p01");
    let pages: Vec<_> = parse_ok(&name, &bytes).pages().collect();
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].transition, PageTransition::Global);
    assert_eq!(
        pages[1].transition,
        PageTransition::Effect {
            kind: TransitionKind::LeftToRight,
            duration_ms: 2000
        }
    );
    assert_eq!((pages[1].width, pages[1].height), (1920, 1080));
}
