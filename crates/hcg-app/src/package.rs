//! Input and output packages as flat file lists. Adapters turn a zip into [`InputFile`]s and
//! [`OutputFile`]s back into a zip; the rules for grouping files into bundles and laying out the
//! output live here, as pure functions.
//!
//! Paths use `/` as separator, have no leading `/`, and contain no empty, `.` or `..` components
//! (adapters normalize them).

use std::collections::BTreeMap;

use crate::bundle::{Bundle, BundleFile};

/// How an entry name was decoded from the input archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameEncoding {
    /// Plain ASCII; every encoding agrees.
    Ascii,
    /// The archive marked the name as UTF-8.
    Utf8Flag,
    /// Not marked, but valid UTF-8 (e.g. 7-Zip, macOS).
    Utf8Guessed,
    /// Not marked and not UTF-8: decoded as CP949 (Korean Windows default).
    Cp949,
}

/// One file of the input package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputFile {
    pub path: String,
    pub bytes: Vec<u8>,
    pub name_encoding: NameEncoding,
}

/// One file of the output package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputFile {
    pub path: String,
    pub bytes: Vec<u8>,
}

/// Result of grouping input files into bundles.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Pairing {
    /// One bundle per `.hcg` file, sorted by hcg path.
    pub bundles: Vec<Bundle>,
    /// Files that belong to no hcg (shown in the report), sorted.
    pub unpaired: Vec<String>,
    /// OS clutter that is dropped silently (`__MACOSX/`, `.DS_Store`, `Thumbs.db`, ...), sorted.
    pub ignored: Vec<String>,
}

/// Groups files into bundles.
///
/// - Every `*.hcg` (extension case-insensitive) starts a bundle.
/// - Its image folder is the directory next to it with the same name as the hcg stem, matched
///   case-insensitively like Windows. Only direct children of that folder become images.
/// - Files at any depth work, so an extra wrapping folder in the zip is fine.
/// - An hcg without a folder gives a bundle with `folder_path: None` and no images.
pub fn pair_bundles(files: Vec<InputFile>) -> Pairing {
    let mut pairing = Pairing::default();
    let (mut hcgs, mut others) = (Vec::new(), Vec::new());
    for file in files {
        if is_clutter(&file.path) {
            pairing.ignored.push(file.path);
        } else if extension_is(&file.path, "hcg") {
            hcgs.push(file);
        } else {
            others.push(file);
        }
    }

    // Image folder path (lower-cased) -> index of the bundle that owns it.
    let mut folders: BTreeMap<String, usize> = BTreeMap::new();
    hcgs.sort_by(|a, b| a.path.cmp(&b.path));
    for (i, hcg) in hcgs.into_iter().enumerate() {
        let (dir, name) = split_parent(&hcg.path);
        let stem = name[..name.len() - ".hcg".len()].to_string();
        let folder = join(dir, &stem);
        folders.entry(folder.to_lowercase()).or_insert(i);
        pairing.bundles.push(Bundle {
            stem,
            hcg_path: hcg.path,
            folder_path: None,
            hcg: hcg.bytes,
            images: Vec::new(),
        });
    }

    for file in others {
        let (dir, name) = split_parent(&file.path);
        match folders.get(&dir.to_lowercase()) {
            Some(&i) => {
                let bundle = &mut pairing.bundles[i];
                bundle.folder_path.get_or_insert_with(|| dir.to_string());
                bundle.images.push(BundleFile {
                    name: name.to_string(),
                    bytes: file.bytes,
                });
            }
            None => pairing.unpaired.push(file.path),
        }
    }

    for bundle in &mut pairing.bundles {
        bundle.images.sort_by(|a, b| a.name.cmp(&b.name));
    }
    pairing.unpaired.sort();
    pairing.ignored.sort();
    pairing
}

/// One bundle's share of the output: the optimized bundle, or `None` if it could not be
/// processed (then only the report is written), plus its report text.
#[derive(Debug, Clone, Copy)]
pub struct PackagedBundle<'a> {
    pub stem: &'a str,
    pub bundle: Option<&'a Bundle>,
    pub report: &'a str,
}

/// Lays out the output package. Each bundle goes into its own `<stem>_optimized/` folder with
/// `<stem>.hcg`, the image folder `<stem>/` (if it has images), and `report.json`; a bundle that
/// could not be processed gets only `report.json`. Extracting the result next to the original
/// therefore never overwrites it. Folder names are made unique if two bundles share a stem.
pub fn output_files(results: &[PackagedBundle<'_>]) -> Vec<OutputFile> {
    let mut used: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for result in results {
        let base = format!("{}_optimized", result.stem);
        let mut root = base.clone();
        let mut n = 2;
        while used.iter().any(|u| u.eq_ignore_ascii_case(&root)) {
            root = format!("{base}_{n}");
            n += 1;
        }
        used.push(root.clone());

        if let Some(bundle) = result.bundle {
            out.push(OutputFile {
                path: format!("{root}/{}.hcg", bundle.stem),
                bytes: bundle.hcg.clone(),
            });
            for image in &bundle.images {
                out.push(OutputFile {
                    path: format!("{root}/{}/{}", bundle.stem, image.name),
                    bytes: image.bytes.clone(),
                });
            }
        }
        out.push(OutputFile {
            path: format!("{root}/report.json"),
            bytes: result.report.as_bytes().to_vec(),
        });
    }
    out
}

fn is_clutter(path: &str) -> bool {
    let (_, name) = split_parent(path);
    path.split('/').any(|c| c == "__MACOSX")
        || name.starts_with("._")
        || [".DS_Store", "Thumbs.db", "desktop.ini"]
            .iter()
            .any(|n| name.eq_ignore_ascii_case(n))
}

fn extension_is(path: &str, ext: &str) -> bool {
    let (_, name) = split_parent(path);
    name.rsplit_once('.')
        .is_some_and(|(stem, e)| !stem.is_empty() && e.eq_ignore_ascii_case(ext))
}

/// `("a/b", "c.hcg")` for `"a/b/c.hcg"`; `("", "c.hcg")` for `"c.hcg"`.
fn split_parent(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str) -> InputFile {
        InputFile {
            path: path.into(),
            bytes: path.as_bytes().to_vec(),
            name_encoding: NameEncoding::Ascii,
        }
    }

    fn names(b: &Bundle) -> Vec<&str> {
        b.images.iter().map(|i| i.name.as_str()).collect()
    }

    #[test]
    fn pairs_hcg_with_same_named_folder() {
        let p = pair_bundles(vec![
            file("layered/B.png"),
            file("layered.hcg"),
            file("layered/A.jpg"),
            file("notes.txt"),
        ]);
        assert_eq!(p.bundles.len(), 1);
        let b = &p.bundles[0];
        assert_eq!(
            (b.stem.as_str(), b.hcg_path.as_str()),
            ("layered", "layered.hcg")
        );
        assert_eq!(b.folder_path.as_deref(), Some("layered"));
        assert_eq!(b.hcg, b"layered.hcg");
        assert_eq!(names(b), ["A.jpg", "B.png"]);
        assert_eq!(p.unpaired, ["notes.txt"]);
    }

    #[test]
    fn folder_match_is_case_insensitive_and_wrapping_folder_is_fine() {
        let p = pair_bundles(vec![file("Church/Sunday.HCG"), file("Church/SUNDAY/1.jpg")]);
        let b = &p.bundles[0];
        assert_eq!(b.stem, "Sunday");
        assert_eq!(b.folder_path.as_deref(), Some("Church/SUNDAY"));
        assert_eq!(names(b), ["1.jpg"]);
        assert!(p.unpaired.is_empty());
    }

    #[test]
    fn hangul_names_pair() {
        let p = pair_bundles(vec![file("찬양.hcg"), file("찬양/0D373705배경.jpg")]);
        assert_eq!(names(&p.bundles[0]), ["0D373705배경.jpg"]);
    }

    #[test]
    fn several_hcg_files_give_several_bundles() {
        let p = pair_bundles(vec![
            file("b.hcg"),
            file("a.hcg"),
            file("a/x.png"),
            file("b/y.png"),
        ]);
        let stems: Vec<_> = p
            .bundles
            .iter()
            .map(|b| (b.stem.as_str(), names(b)))
            .collect();
        assert_eq!(stems, [("a", vec!["x.png"]), ("b", vec!["y.png"])]);
    }

    #[test]
    fn hcg_without_folder_and_nested_files() {
        let p = pair_bundles(vec![file("x.hcg"), file("y.hcg"), file("y/sub/deep.png")]);
        assert!(
            p.bundles
                .iter()
                .all(|b| b.folder_path.is_none() && b.images.is_empty())
        );
        assert_eq!(p.unpaired, ["y/sub/deep.png"]);
    }

    #[test]
    fn clutter_is_ignored() {
        let p = pair_bundles(vec![
            file("a.hcg"),
            file("a/Thumbs.db"),
            file("__MACOSX/a/._x.png"),
            file("a/._x.png"),
            file(".DS_Store"),
            file("a/x.png"),
        ]);
        assert_eq!(names(&p.bundles[0]), ["x.png"]);
        assert_eq!(p.ignored.len(), 4);
        assert!(p.unpaired.is_empty());
    }

    #[test]
    fn a_file_named_hcg_without_stem_is_not_a_bundle() {
        let p = pair_bundles(vec![file(".hcg"), file("dir.hcg/x")]);
        assert!(p.bundles.is_empty());
        assert_eq!(p.unpaired, [".hcg", "dir.hcg/x"]);
    }

    #[test]
    fn output_layout() {
        let bundle = |stem: &str, images: &[&str]| Bundle {
            stem: stem.into(),
            hcg_path: format!("{stem}.hcg"),
            folder_path: None,
            hcg: vec![1],
            images: images
                .iter()
                .map(|n| BundleFile {
                    name: (*n).into(),
                    bytes: vec![2],
                })
                .collect(),
        };
        let (a, big_a) = (bundle("a", &["1.jpg"]), bundle("A", &[]));
        let out = output_files(&[
            PackagedBundle {
                stem: "a",
                bundle: Some(&a),
                report: "report a",
            },
            PackagedBundle {
                stem: "A",
                bundle: Some(&big_a),
                report: "report A",
            },
            PackagedBundle {
                stem: "broken",
                bundle: None,
                report: "could not read",
            },
        ]);
        let paths: Vec<_> = out.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "a_optimized/a.hcg",
                "a_optimized/a/1.jpg",
                "a_optimized/report.json",
                "A_optimized_2/A.hcg",
                "A_optimized_2/report.json",
                "broken_optimized/report.json",
            ]
        );
        assert_eq!(out[2].bytes, b"report a");
    }
}
