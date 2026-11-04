//! apply: carry out a [`BundlePlan`] (issue #9). Shrinks the planned files, rewrites hcg paths
//! and folder names when an extension changes, writes the hcg, and verifies the result by
//! parsing it again.

use std::collections::{HashMap, HashSet};

use hcg_format::{HcgFile, ImageSlot, ImageSource, ParseError, parse, subfolder_file_name};

use crate::analyze::BundlePlan;
use crate::bundle::{Bundle, BundleFile};
use crate::policy::{
    ImageAction, ImageOutcome, PlannedAction, ResizePolicy, optimize_image, output_file_name,
};
use crate::ports::{ImageCodec, Progress};

/// The user cancelled; no output was produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("cancelled")]
pub struct Cancelled;

/// What happened to one planned-for-shrinking folder file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileResult {
    /// Original folder file name.
    pub name: String,
    /// Folder file name in the output (differs when the extension changed).
    pub new_name: String,
    /// New path written into the hcg, if it changed.
    pub new_path: Option<String>,
    pub outcome: ImageOutcome,
}

/// Why a bundle produced no output.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApplyFailure {
    #[error("the hcg file could not be read: {0}")]
    Parse(ParseError),
    #[error("an image path could not be updated: {0}")]
    Edit(String),
    #[error("the rewritten hcg failed verification: {0}")]
    Verify(String),
}

/// The result of applying one bundle's plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleResult {
    pub plan: BundlePlan,
    /// The optimized bundle, or `None` if the bundle failed (see `failure`).
    pub output: Option<Bundle>,
    /// One entry per file the plan marked for shrinking, in plan order.
    pub files: Vec<FileResult>,
    pub failure: Option<ApplyFailure>,
}

/// Applies a plan. Checks for cancellation between images (contract A10). On any failure the
/// bundle gets no output and the reason is recorded (A1, A9).
pub fn apply<C: ImageCodec, P: Progress>(
    codec: &C,
    bundle: &Bundle,
    plan: BundlePlan,
    policy: &ResizePolicy,
    progress: &mut P,
) -> Result<BundleResult, Cancelled> {
    let failed = |plan: BundlePlan, files, failure| {
        Ok(BundleResult {
            plan,
            output: None,
            files,
            failure: Some(failure),
        })
    };
    if let Err(e) = &plan.hcg {
        let failure = ApplyFailure::Parse(e.clone());
        return failed(plan, Vec::new(), failure);
    }
    let before = match parse(&bundle.hcg) {
        Ok(file) => file,
        Err(e) => return failed(plan, Vec::new(), ApplyFailure::Parse(e)),
    };
    let mut hcg = before.clone();

    let mut images = bundle.images.clone();
    let index: HashMap<String, usize> = images
        .iter()
        .enumerate()
        .map(|(i, f)| (f.name.to_lowercase(), i))
        .collect();
    let mut taken: HashSet<String> = images.iter().map(|f| f.name.to_lowercase()).collect();
    let mut changes: HashMap<ImageSlot, (String, String, String)> = HashMap::new(); // old path, new path, new name
    let mut results = Vec::new();

    let total = plan.shrink_count();
    let mut done = 0;
    for file_plan in &plan.files {
        if !matches!(file_plan.action, PlannedAction::Shrink { .. }) {
            continue;
        }
        if progress.is_cancelled() {
            return Err(Cancelled);
        }
        progress.report(done, total, &file_plan.name);
        let Some(&i) = index.get(&file_plan.name.to_lowercase()) else {
            continue; // cannot happen: the plan was built from this bundle
        };
        let outcome = optimize_image(codec, &file_plan.name, &images[i].bytes, policy);
        let mut result = FileResult {
            name: file_plan.name.clone(),
            new_name: file_plan.name.clone(),
            new_path: None,
            outcome,
        };
        if let ImageAction::Converted(converted) = &result.outcome.action {
            let old_path = match file_plan.refs.first().map(|r| &r.source) {
                Some(ImageSource::Path(p)) => p.clone(),
                _ => String::new(),
            };
            let stamp = file_plan.refs.first().map_or(0, |r| r.stamp);
            let (dir, file_name) = split_path(&old_path);
            let (new_file_name, extension_changed) = output_file_name(file_name, converted.format);
            if extension_changed {
                let (new_path, new_name) =
                    unique_name(dir, &new_file_name, stamp, &taken, &file_plan.name);
                for image_ref in &file_plan.refs {
                    if let Err(e) = hcg.set_image_path(image_ref.slot, &new_path) {
                        return failed(plan, results, ApplyFailure::Edit(e.to_string()));
                    }
                    changes.insert(
                        image_ref.slot,
                        (old_path.clone(), new_path.clone(), new_name.clone()),
                    );
                }
                taken.remove(&file_plan.name.to_lowercase());
                taken.insert(new_name.to_lowercase());
                images[i].name = new_name.clone();
                result.new_name = new_name;
                result.new_path = Some(new_path);
            }
            images[i].bytes = converted.bytes.clone();
        }
        results.push(result);
        done += 1;
    }
    progress.report(total, total, "");

    let hcg_bytes = hcg.to_bytes();
    let found_before: HashSet<ImageSlot> = plan
        .files
        .iter()
        .flat_map(|f| f.refs.iter().map(|r| r.slot))
        .collect();
    if let Err(message) = verify(
        &before,
        bundle.hcg.len(),
        &hcg_bytes,
        &changes,
        &found_before,
        &images,
    ) {
        return failed(plan, results, ApplyFailure::Verify(message));
    }

    images.sort_by(|a, b| a.name.cmp(&b.name));
    let output = Bundle {
        stem: bundle.stem.clone(),
        hcg_path: bundle.hcg_path.clone(),
        folder_path: bundle.folder_path.clone(),
        hcg: hcg_bytes,
        images,
    };
    Ok(BundleResult {
        plan,
        output: Some(output),
        files: results,
        failure: None,
    })
}

/// `("C:\\test\\", "noise.png")` for `C:\test\noise.png`. Accepts `\` and `/`.
fn split_path(path: &str) -> (&str, &str) {
    match path.rfind(['\\', '/']) {
        Some(i) => path.split_at(i + 1),
        None => ("", path),
    }
}

/// New path and folder file name for `dir + file_name`, adding `_1`, `_2`, … to the file stem
/// if the folder name would collide with another file (contract A5).
fn unique_name(
    dir: &str,
    file_name: &str,
    stamp: u64,
    taken: &HashSet<String>,
    current: &str,
) -> (String, String) {
    let (stem, ext) = file_name.rsplit_once('.').unwrap_or((file_name, ""));
    let dot_ext = if ext.is_empty() {
        String::new()
    } else {
        format!(".{ext}")
    };
    let mut n = 0u32;
    loop {
        let candidate = if n == 0 {
            file_name.to_string()
        } else {
            format!("{stem}_{n}{dot_ext}")
        };
        let path = format!("{dir}{candidate}");
        let name = subfolder_file_name(&path, stamp);
        let lower = name.to_lowercase();
        if !taken.contains(&lower) || lower == current.to_lowercase() {
            return (path, name);
        }
        n += 1;
    }
}

/// Re-parses the rewritten hcg and checks contracts A1 and A2.
fn verify(
    before: &HcgFile,
    before_len: usize,
    after_bytes: &[u8],
    changes: &HashMap<ImageSlot, (String, String, String)>,
    found_before: &HashSet<ImageSlot>,
    output_images: &[BundleFile],
) -> Result<(), String> {
    let after = parse(after_bytes).map_err(|e| format!("cannot re-read the result: {e}"))?;
    if after.version() != before.version()
        || after.page_count() != before.page_count()
        || after.global_object_count() != before.global_object_count()
        || !after.pages().eq(before.pages())
    {
        return Err("page structure changed".into());
    }
    let (old_images, new_images) = (before.images(), after.images());
    if old_images.len() != new_images.len() {
        return Err("number of images changed".into());
    }
    let names: HashSet<String> = output_images
        .iter()
        .map(|f| f.name.to_lowercase())
        .collect();
    let mut expected_len = before_len as i64;
    for (old, new) in old_images.iter().zip(&new_images) {
        let same_place = (
            old.slot, old.page, old.layer, old.object, old.role, old.stamp,
        ) == (
            new.slot, new.page, new.layer, new.object, new.role, new.stamp,
        );
        if !same_place {
            return Err(format!("image {:?} moved or changed", old.slot));
        }
        match changes.get(&old.slot) {
            Some((old_path, new_path, new_name)) => {
                if new.source != ImageSource::Path(new_path.clone())
                    || new.folder_file_name.as_deref() != Some(new_name.as_str())
                {
                    return Err(format!("image path not updated to {new_path}"));
                }
                let units = |s: &str| s.encode_utf16().count() as i64;
                expected_len += 2 * (units(new_path) - units(old_path));
            }
            None if old != new => return Err(format!("image {:?} changed unexpectedly", old.slot)),
            None => {}
        }
        if found_before.contains(&new.slot) {
            let resolves = new
                .folder_file_name
                .as_ref()
                .is_some_and(|n| names.contains(&n.to_lowercase()));
            if !resolves {
                return Err(format!(
                    "image {:?} no longer resolves to a folder file",
                    new.slot
                ));
            }
        }
    }
    if after_bytes.len() as i64 != expected_len {
        return Err("file size differs beyond the changed paths".into());
    }
    Ok(())
}
