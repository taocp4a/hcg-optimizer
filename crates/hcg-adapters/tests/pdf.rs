//! Page overview PDF over every sample (issue #19).

use std::path::{Path, PathBuf};

use hcg_adapters::pdf::{Thumbnails, overview_pdf};
use hcg_app::bundle::Bundle;
use hcg_app::overview::{OverviewSettings, PerSheet};

fn samples() -> Vec<Bundle> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../project/samples");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&root)
        .expect("samples")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "hcg"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let stem = path
                .file_stem()
                .expect("stem")
                .to_string_lossy()
                .into_owned();
            Bundle {
                hcg_path: format!("{stem}.hcg"),
                stem,
                folder_path: None,
                hcg: std::fs::read(&path).expect("read"),
                images: Vec::new(),
            }
        })
        .collect()
}

fn count(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|w| *w == needle)
        .count()
}

#[test]
fn every_sample_gives_one_image_per_page() {
    let bundles = samples();
    assert!(bundles.len() > 50);
    for bundle in &bundles {
        let thumbnails =
            Thumbnails::from_bundle(bundle).unwrap_or_else(|e| panic!("{}: {e}", bundle.stem));
        for per_sheet in PerSheet::ALL {
            let settings = OverviewSettings { per_sheet };
            let overview = thumbnails.layout(&settings);
            let pdf = overview_pdf(&thumbnails, &settings);
            let name = &bundle.stem;
            assert!(pdf.bytes.starts_with(b"%PDF-"), "{name}");
            assert!(
                pdf.bytes.ends_with(b"%%EOF\n") || pdf.bytes.ends_with(b"%%EOF"),
                "{name}"
            );
            assert!(pdf.blank_pages.is_empty(), "{name}: {:?}", pdf.blank_pages);
            let sheets = overview.page_count.div_ceil(per_sheet.count()).max(1);
            assert_eq!(overview.sheets.len(), sheets, "{name}");
            assert_eq!(
                (pdf.sheets, pdf.page_count),
                (sheets, overview.page_count),
                "{name}"
            );
            let pages = count(&pdf.bytes, b"/Type /Page") - count(&pdf.bytes, b"/Type /Pages");
            assert_eq!(pages, sheets, "{name}: PDF pages");
            assert_eq!(
                count(&pdf.bytes, b"/Subtype /Image"),
                overview.page_count,
                "{name}"
            );
            let cells: Vec<usize> = overview
                .sheets
                .iter()
                .flat_map(|s| s.cells.iter().map(|c| c.page))
                .collect();
            assert_eq!(
                cells,
                (1..=overview.page_count).collect::<Vec<_>>(),
                "{name}"
            );
        }
    }
}

#[test]
fn title_keeps_non_ascii_names() {
    let mut bundle = samples().into_iter().next().expect("a sample");
    bundle.stem = "주일 3부".into();
    let thumbnails = Thumbnails::from_bundle(&bundle).expect("thumbnails");
    let overview = thumbnails.layout(&OverviewSettings::default());
    assert!(overview.sheets.iter().all(|s| s.header.is_ascii()));
    let pdf = overview_pdf(&thumbnails, &OverviewSettings::default());
    assert_eq!(pdf.file_name, "주일 3부_pages.pdf");
    // PDF text strings outside PDFDocEncoding are UTF-16BE with a byte-order mark (hex here).
    let hex: String = std::iter::once("<FEFF".to_string())
        .chain("주일".encode_utf16().map(|u| format!("{u:04X}")))
        .collect();
    let text = String::from_utf8_lossy(&pdf.bytes).to_uppercase();
    assert!(text.contains(&hex), "UTF-16 title {hex}");
}

#[test]
fn unreadable_hcg_is_an_error() {
    let mut bundle = samples().into_iter().next().expect("a sample");
    bundle.hcg.truncate(100);
    assert!(Thumbnails::from_bundle(&bundle).is_err());
}
