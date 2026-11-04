//! Contract tests for the zip adapter (issue #5, contracts Z1–Z7).

use std::io::Cursor;
use std::path::{Path, PathBuf};

use hcg_adapters::archive::{ArchiveError, ZipLimits, read_zip, write_zip};
use hcg_app::package::{
    InputFile, NameEncoding, OutputFile, PackagedBundle, output_files, pair_bundles,
};
use zip::{CompressionMethod, ZipArchive};

fn samples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../project/samples")
}

/// `<stem>.hcg` and every file of `<stem>/`, as output files under `prefix`.
fn sample_files(stem: &str, prefix: &str) -> Vec<OutputFile> {
    let dir = samples_dir();
    let mut out = vec![OutputFile {
        path: format!("{prefix}{stem}.hcg"),
        bytes: std::fs::read(dir.join(format!("{stem}.hcg"))).expect("hcg"),
    }];
    if let Ok(entries) = std::fs::read_dir(dir.join(stem)) {
        let mut names: Vec<_> = entries.map(|e| e.expect("entry").file_name()).collect();
        names.sort();
        for name in names {
            let name = name.to_string_lossy().into_owned();
            out.push(OutputFile {
                path: format!("{prefix}{stem}/{name}"),
                bytes: std::fs::read(dir.join(stem).join(&name)).expect("image"),
            });
        }
    }
    out
}

fn read(bytes: &[u8]) -> Result<Vec<InputFile>, ArchiveError> {
    read_zip(bytes, &ZipLimits::default())
}

/// Minimal zip writer for crafted entries (raw name bytes, chosen flags, stored data).
fn craft_zip(entries: &[(&[u8], u16, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for &(name, flags, data) in entries {
        let offset = out.len() as u32;
        let crc = crc32fast::hash(data);
        let n = name.len() as u16;
        let len = data.len() as u32;
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // stored
        out.extend_from_slice(&0u32.to_le_bytes()); // time, date
        for v in [crc, len, len] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra
        out.extend_from_slice(name);
        out.extend_from_slice(data);

        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes()); // made by (DOS)
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&flags.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        for v in [crc, len, len] {
            central.extend_from_slice(&v.to_le_bytes());
        }
        central.extend_from_slice(&n.to_le_bytes());
        central.extend_from_slice(&[0; 8]); // extra, comment, disk, internal attrs
        central.extend_from_slice(&0u32.to_le_bytes()); // external attrs
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name);
    }
    let cd_offset = out.len() as u32;
    let count = entries.len() as u16;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]); // disk numbers
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

const UTF8: u16 = 1 << 11;

/// Z3: the user's simulated Windows Explorer zip (CP949 name, no UTF-8 flag).
#[test]
fn z3_reads_windows_cp949_zip() {
    let bytes = std::fs::read(samples_dir().join("cp949_sample_SIMULATED.zip")).expect("sample");
    let files = read(&bytes).expect("read");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "찬양배경/주님의 은혜.jpg");
    assert_eq!(files[0].name_encoding, NameEncoding::Cp949);
    assert_eq!(&files[0].bytes[..3], &[0xFF, 0xD8, 0xFF], "JPEG data");
}

/// Z3: name decoding and path normalization rules.
#[test]
fn z3_entry_names() {
    let cp949 = encoding_rs::EUC_KR.encode("찬양/배경.jpg").0.into_owned();
    let zip = craft_zip(&[
        (b"plain.txt", 0, b"a"),
        ("한글/flag.txt".as_bytes(), UTF8, b"b"),
        ("한글/noflag.txt".as_bytes(), 0, b"c"),
        (&cp949, 0, b"d"),
        (b"dir\\sub\\win.txt", 0, b"e"),
        (b"./lead/./dot.txt", 0, b"f"),
        (b"folder/", 0, b""),
    ]);
    let files = read(&zip).expect("read");
    let got: Vec<_> = files
        .iter()
        .map(|f| (f.path.as_str(), f.name_encoding, f.bytes.as_slice()))
        .collect();
    assert_eq!(
        got,
        [
            ("plain.txt", NameEncoding::Ascii, &b"a"[..]),
            ("한글/flag.txt", NameEncoding::Utf8Flag, b"b"),
            ("한글/noflag.txt", NameEncoding::Utf8Guessed, b"c"),
            ("찬양/배경.jpg", NameEncoding::Cp949, b"d"),
            ("dir/sub/win.txt", NameEncoding::Ascii, b"e"),
            ("lead/dot.txt", NameEncoding::Ascii, b"f"),
        ]
    );
}

#[test]
fn z3_unsafe_and_invalid_entries_are_errors() {
    for name in [
        &b"../evil.txt"[..],
        b"a/../../evil.txt",
        b"C:/evil.txt",
        b".",
    ] {
        let err = read(&craft_zip(&[(name, 0, b"x")])).expect_err("unsafe");
        assert!(matches!(err, ArchiveError::UnsafePath { .. }), "{err}");
    }
    let err = read(&craft_zip(&[(&[0xFF, 0xFE], UTF8, b"x")])).expect_err("bad utf8");
    assert!(matches!(err, ArchiveError::UndecodableName { .. }), "{err}");
    let err = read(&craft_zip(&[(b"a.txt", 0, b"1"), (b"a.txt", 0, b"2")])).expect_err("dup");
    assert!(
        matches!(
            err,
            ArchiveError::DuplicateEntries {
                declared: 2,
                distinct: 1
            }
        ),
        "{err}"
    );
    // Different raw names that normalize to the same path.
    let err = read(&craft_zip(&[(b"a/b.txt", 0, b"1"), (b"a\\b.txt", 0, b"2")])).expect_err("dup");
    assert!(matches!(err, ArchiveError::DuplicatePath { .. }), "{err}");
    let err = read(&craft_zip(&[(b"secret.txt", 1, b"x")])).expect_err("encrypted");
    assert!(matches!(err, ArchiveError::Encrypted { .. }), "{err}");
    assert!(matches!(
        read(b"not a zip").expect_err("garbage"),
        ArchiveError::Invalid(_)
    ));
}

/// Z2: limits are enforced on bytes actually inflated.
#[test]
fn z2_zip_bomb_and_entry_limits() {
    let bomb = write_zip(&[OutputFile {
        path: "zeros.bin".into(),
        bytes: vec![0; 16 << 20],
    }])
    .expect("write");
    assert!(
        bomb.len() < 100 << 10,
        "16 MiB of zeros should compress well"
    );
    let limits = ZipLimits {
        max_entries: 10,
        max_total_bytes: 1 << 20,
    };
    let err = read_zip(&bomb, &limits).expect_err("bomb");
    assert!(
        matches!(err, ArchiveError::TooLarge { limit } if limit == 1 << 20),
        "{err}"
    );

    let many: Vec<_> = (0..11)
        .map(|i| OutputFile {
            path: format!("{i}.txt"),
            bytes: vec![],
        })
        .collect();
    let err = read_zip(&write_zip(&many).expect("write"), &limits).expect_err("entries");
    assert!(
        matches!(err, ArchiveError::TooManyEntries { limit: 10 }),
        "{err}"
    );
}

/// Z1: truncated or corrupted zips never panic.
#[test]
fn z1_malformed_zips_never_panic() {
    let zip = write_zip(&sample_files("v11_two_images", "")).expect("write");
    for cut in 0..zip.len() {
        let _ = read(&zip[..cut]);
    }
    // Mutate the local headers, the start of the data, and the whole central directory.
    let central = zip.len() - 400;
    let offsets: Vec<usize> = (0..400).chain(central..zip.len()).collect();
    let mut buf = zip.clone();
    for at in offsets {
        for value in [0x00, 0xFF, 0x7F] {
            let original = buf[at];
            buf[at] = value;
            let _ = read(&buf);
            buf[at] = original;
        }
    }
}

/// Z4: write -> read returns the same paths and bytes (including Hangul); Z7: methods.
#[test]
fn z4_z7_roundtrip_and_compression() {
    let mut files = sample_files("layered", "");
    files.extend(sample_files("v10_bg", "주일예배/"));
    files.push(OutputFile {
        path: "주일예배/보고서.txt".into(),
        bytes: "한글 내용".into(),
    });
    let zip = write_zip(&files).expect("write");

    let back = read(&zip).expect("read");
    let pairs: Vec<_> = back
        .iter()
        .map(|f| (f.path.clone(), f.bytes.clone()))
        .collect();
    let expected: Vec<_> = files
        .iter()
        .map(|f| (f.path.clone(), f.bytes.clone()))
        .collect();
    assert_eq!(pairs, expected);
    for f in &back {
        let expected = if f.path.is_ascii() {
            NameEncoding::Ascii
        } else {
            NameEncoding::Utf8Flag
        };
        assert_eq!(f.name_encoding, expected, "{}", f.path);
    }

    let mut archive = ZipArchive::new(Cursor::new(&zip)).expect("zip");
    for i in 0..archive.len() {
        let entry = archive.by_index_raw(i).expect("entry");
        let name = entry.name().expect("name").to_lowercase();
        let expected = if name.ends_with(".jpg") || name.ends_with(".png") {
            CompressionMethod::Stored
        } else {
            CompressionMethod::Deflated
        };
        assert_eq!(entry.compression(), expected, "{name}");
    }
}

/// Z6: the same files always give the same zip bytes.
#[test]
fn z6_output_is_deterministic() {
    let files = sample_files("v11_two_images", "x/");
    assert_eq!(write_zip(&files).expect("a"), write_zip(&files).expect("b"));
}

/// Z5 end to end: a zip with a wrapping folder, several hcg files and clutter is paired into
/// bundles, and the output layout zips back up.
#[test]
fn z5_zip_to_bundles_and_back() {
    let mut files = sample_files("layered", "주일/");
    files.extend(sample_files("speedtest", "주일/"));
    files.extend(sample_files("v00_empty", ""));
    files.push(OutputFile {
        path: "주일/Thumbs.db".into(),
        bytes: vec![1],
    });
    files.push(OutputFile {
        path: "readme.txt".into(),
        bytes: vec![2],
    });
    let zip = write_zip(&files).expect("write");

    let pairing = pair_bundles(read(&zip).expect("read"));
    let summary: Vec<_> = pairing
        .bundles
        .iter()
        .map(|b| {
            (
                b.hcg_path.as_str(),
                b.folder_path.as_deref(),
                b.images.len(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("v00_empty.hcg", None, 0),
            ("주일/layered.hcg", Some("주일/layered"), 16),
            ("주일/speedtest.hcg", Some("주일/speedtest"), 3),
        ]
    );
    assert_eq!(pairing.unpaired, ["readme.txt"]);
    assert_eq!(pairing.ignored, ["주일/Thumbs.db"]);

    let results: Vec<_> = pairing
        .bundles
        .iter()
        .map(|b| PackagedBundle {
            stem: &b.stem,
            bundle: Some(b),
            report: "report",
        })
        .collect();
    let out = output_files(&results);
    let back = read(&write_zip(&out).expect("write")).expect("read");
    assert_eq!(back.len(), out.len());
    assert!(
        back.iter()
            .any(|f| f.path == "layered_optimized/layered/0D373705big_0.jpg")
    );
    assert!(
        back.iter()
            .any(|f| f.path == "v00_empty_optimized/report.json")
    );
}
