//! analyze: plan a bundle from headers only (issue #9). Parses the hcg, matches image references
//! to folder files, and decides per file what will happen, without decoding any image.

use std::collections::HashMap;

use hcg_format::{
    HcgFile, ImageRef, ImageSource, PageInfo, PageTransition, ParseError, TransitionKind, parse,
};

use crate::bundle::Bundle;
use crate::policy::{KeepReason, PlannedAction, ResizePolicy, plan_image};
use crate::ports::{ImageCodec, ImageProbe};

/// Facts about a parsed hcg.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HcgSummary {
    pub version: i32,
    pub page_count: usize,
    pub image_refs: usize,
}

/// One referenced folder file, however many image references point to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePlan {
    /// File name in the image folder, as found (original case).
    pub name: String,
    /// Every image reference to this file, in file order.
    pub refs: Vec<ImageRef>,
    /// Header facts, if readable.
    pub probe: Option<ImageProbe>,
    pub bytes: u64,
    pub action: PlannedAction,
}

/// Everything analyze learned about one bundle. Also the input to [`crate::apply::apply`] and
/// the data behind the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundlePlan {
    pub stem: String,
    pub hcg_path: String,
    pub folder_path: Option<String>,
    /// The parsed hcg's facts, or why it could not be read.
    pub hcg: Result<HcgSummary, ParseError>,
    /// Referenced folder files, in order of first reference.
    pub files: Vec<FilePlan>,
    /// Path references whose folder file is missing. ez-CGER can still show these only on a PC
    /// where the original path exists.
    pub missing: Vec<ImageRef>,
    /// Embedded images and empty blocks (never touched).
    pub embedded: Vec<ImageRef>,
    /// Pages with their own transition effect other than CUT.
    pub transitions: Vec<PageInfo>,
    /// Folder files no image refers to (copied unchanged), sorted.
    pub unreferenced: Vec<String>,
}

impl BundlePlan {
    /// Number of files apply will decode and re-encode.
    pub fn shrink_count(&self) -> usize {
        self.files
            .iter()
            .filter(|f| matches!(f.action, PlannedAction::Shrink { .. }))
            .count()
    }
}

/// Plans a bundle. Never decodes an image (contract A8) and never fails: a broken hcg is
/// recorded in [`BundlePlan::hcg`].
pub fn analyze<C: ImageCodec>(codec: &C, bundle: &Bundle, policy: &ResizePolicy) -> BundlePlan {
    match parse(&bundle.hcg) {
        Ok(file) => analyze_parsed(codec, bundle, &file, policy),
        Err(e) => BundlePlan {
            stem: bundle.stem.clone(),
            hcg_path: bundle.hcg_path.clone(),
            folder_path: bundle.folder_path.clone(),
            hcg: Err(e),
            files: Vec::new(),
            missing: Vec::new(),
            embedded: Vec::new(),
            transitions: Vec::new(),
            unreferenced: Vec::new(),
        },
    }
}

pub(crate) fn analyze_parsed<C: ImageCodec>(
    codec: &C,
    bundle: &Bundle,
    file: &HcgFile<'_>,
    policy: &ResizePolicy,
) -> BundlePlan {
    // Folder lookup is case-insensitive, like Windows.
    let index: HashMap<String, usize> = bundle
        .images
        .iter()
        .enumerate()
        .map(|(i, f)| (f.name.to_lowercase(), i))
        .collect();
    let mut files: Vec<FilePlan> = Vec::new();
    let mut plan_of_image: HashMap<usize, usize> = HashMap::new();
    let (mut missing, mut embedded) = (Vec::new(), Vec::new());

    let images = file.images();
    let image_refs = images.len();
    for image in images {
        if !matches!(image.source, ImageSource::Path(_)) {
            embedded.push(image);
            continue;
        }
        let found = image
            .folder_file_name
            .as_ref()
            .and_then(|n| index.get(&n.to_lowercase()));
        let Some(&i) = found else {
            missing.push(image);
            continue;
        };
        let plan_index = match plan_of_image.get(&i) {
            Some(&p) => p,
            None => {
                let folder_file = &bundle.images[i];
                let bytes = folder_file.bytes.len() as u64;
                let (probe, action) = match codec.probe(&folder_file.bytes, &folder_file.name) {
                    Ok(probe) => (Some(probe), plan_image(&probe, bytes, policy)),
                    Err(e) => (
                        None,
                        PlannedAction::Keep(KeepReason::Undecodable(e.to_string())),
                    ),
                };
                files.push(FilePlan {
                    name: folder_file.name.clone(),
                    refs: Vec::new(),
                    probe,
                    bytes,
                    action,
                });
                plan_of_image.insert(i, files.len() - 1);
                files.len() - 1
            }
        };
        files[plan_index].refs.push(image);
    }

    let mut unreferenced: Vec<String> = bundle
        .images
        .iter()
        .enumerate()
        .filter(|(i, _)| !plan_of_image.contains_key(i))
        .map(|(_, f)| f.name.clone())
        .collect();
    unreferenced.sort();

    let transitions = file
        .pages()
        .filter(|p| {
            matches!(p.transition, PageTransition::Effect { kind, .. } if kind != TransitionKind::Cut)
        })
        .collect();

    BundlePlan {
        stem: bundle.stem.clone(),
        hcg_path: bundle.hcg_path.clone(),
        folder_path: bundle.folder_path.clone(),
        hcg: Ok(HcgSummary {
            version: file.version(),
            page_count: file.page_count(),
            image_refs,
        }),
        files,
        missing,
        embedded,
        transitions,
        unreferenced,
    }
}
