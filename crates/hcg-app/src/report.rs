//! Inspection report (issue #11): a [`BundleResult`] becomes structured data ([`Report`],
//! serializable as JSON for a viewer). It also carries a one-line headline and a plain-text
//! rendering (`text`) built from the same data; the output zip contains that text as
//! `report.txt`. English for now; every warning has a stable
//! `code` so a viewer can translate.

use serde::Serialize;

use hcg_format::{
    ImageRef, ImageRole, ImageSource, PageTransition, ParseErrorKind, TransitionKind,
};

use crate::apply::{ApplyFailure, BundleResult, FileResult};
use crate::policy::{ImageAction, ImageWarning, KeepReason, PlannedAction, ResizePolicy};
use crate::ports::{ImageFormat, OutputFormat};

/// Report format version.
pub const SCHEMA: u32 = 1;

/// Files in the input that belong to no bundle.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunNotes {
    pub unpaired: Vec<String>,
    pub ignored: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub schema: u32,
    pub tool: ToolInfo,
    pub bundle: BundleInfo,
    pub status: Status,
    pub summary: Summary,
    pub failure: Option<Failure>,
    pub hcg: Option<HcgInfo>,
    pub policy: PolicyInfo,
    pub images: Vec<ImageEntry>,
    pub missing: Vec<MissingEntry>,
    pub embedded: Vec<EmbeddedEntry>,
    pub transitions: Vec<TransitionEntry>,
    pub unreferenced_files: usize,
    pub other_files: OtherFiles,
    pub warnings: Vec<Warning>,
    /// The whole report as plain text (lines separated by `\n`), rendered from the fields above.
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BundleInfo {
    pub stem: String,
    pub hcg_path: String,
    pub folder_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Summary {
    /// One sentence, e.g. "2 of 3 images shrunk: 29.5 MB -> 2.6 MB. 1 warning."
    pub headline: String,
    pub images_total: usize,
    pub images_shrunk: usize,
    pub images_renamed: usize,
    /// Total size of the referenced folder files before and after.
    pub bytes_before: u64,
    pub bytes_after: u64,
    pub warnings: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Failure {
    pub code: String,
    pub message: String,
    pub advice: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HcgInfo {
    pub version: i32,
    pub pages: usize,
    pub image_refs: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PolicyInfo {
    pub max_width: u32,
    pub max_height: u32,
    pub max_bytes: u64,
    pub jpeg_quality: u8,
    pub min_jpeg_quality: u8,
}

/// Where an image is used. All numbers are 1-based; `page` is `null` for the background layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UsedOn {
    pub page: Option<usize>,
    pub layer: usize,
    pub object: usize,
    pub role: Role,
    /// Position in the object's image list (1-based) when `role` is `sequence`.
    pub frame: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Primary,
    Sequence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImageInfo {
    /// `jpeg`, `png`, `bmp`, `gif`, `dds`, `tga`, `webp`, `heic`, `other`.
    pub format: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub bytes: u64,
    /// JPEG quality, for converted JPEG output.
    pub quality: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Shrunk,
    Kept,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Renamed {
    pub file: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImageEntry {
    /// File name in the image folder (`ID + name`).
    pub file: String,
    /// The image path stored in the hcg.
    pub path: String,
    pub used_on: Vec<UsedOn>,
    pub before: ImageInfo,
    pub action: Action,
    /// Why it was kept: `within_limits`, `not_resized`, `not_supported_by_app`, `undecodable`,
    /// `too_many_pixels`, `encode_failed`, `no_gain`.
    pub reason: Option<String>,
    pub after: Option<ImageInfo>,
    pub renamed_to: Option<Renamed>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MissingEntry {
    pub path: String,
    pub expected_file: Option<String>,
    pub used_on: Vec<UsedOn>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EmbeddedEntry {
    /// `embedded` (image bytes inside the hcg) or `empty` (no image data).
    pub kind: String,
    pub bytes: u64,
    pub used_on: UsedOn,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TransitionEntry {
    pub page: usize,
    /// `mix`, `left_to_right`, `right_to_left`, `bottom_to_top`, `top_to_bottom`, `unknown`.
    pub kind: String,
    /// The raw transition type value from the page header.
    pub type_value: i32,
    pub duration_ms: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OtherFiles {
    /// Files in the zip that belong to no hcg.
    pub unpaired: Vec<String>,
    /// OS clutter (`__MACOSX/`, `Thumbs.db`, ...) that was dropped.
    pub ignored: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Warning {
    /// `missing_image`, `not_supported_by_app`, `over_size_limit`, `undecodable`,
    /// `too_many_pixels`, `encode_failed`, `transition_not_cut`, `unsupported_version`,
    /// `parse_failed`, `apply_failed`.
    pub code: String,
    pub message: String,
    /// The image-folder file or path concerned, if any.
    pub file: Option<String>,
}

/// Builds the report for one bundle. Pure and deterministic.
pub fn build_report(result: &BundleResult, run: &RunNotes, policy: &ResizePolicy) -> Report {
    let plan = &result.plan;
    let mut warnings = Vec::new();

    let failure = result.failure.as_ref().map(|f| {
        let (code, advice) = failure_code_and_advice(f);
        let message = sentence(&f.to_string());
        warnings.push(Warning {
            code: code.into(),
            message: message.clone(),
            file: None,
        });
        Failure {
            code: code.into(),
            message,
            advice: advice.into(),
        }
    });

    let images: Vec<ImageEntry> = plan
        .files
        .iter()
        .map(|file| {
            let done = result.files.iter().find(|r| r.name == file.name);
            image_entry(file, done, &mut warnings)
        })
        .collect();

    let mut missing: Vec<MissingEntry> = Vec::new();
    for image in &plan.missing {
        let path = path_of(image);
        match missing.iter_mut().find(|m| m.path == path) {
            Some(entry) => entry.used_on.push(used_on(image)),
            None => missing.push(MissingEntry {
                path: path.clone(),
                expected_file: image.folder_file_name.clone(),
                used_on: vec![used_on(image)],
            }),
        }
    }
    for entry in &missing {
        warnings.push(Warning {
            code: "missing_image".into(),
            message: format!(
                "{} ({}) is not in the image folder. ez-CGER can still show it only on a PC \
                 where that path exists.",
                entry.path,
                places(&entry.used_on)
            ),
            file: entry.expected_file.clone(),
        });
    }

    let embedded = plan
        .embedded
        .iter()
        .map(|image| {
            let (kind, bytes) = match image.source {
                ImageSource::Embedded { len } => ("embedded", len as u64),
                _ => ("empty", 0),
            };
            EmbeddedEntry {
                kind: kind.into(),
                bytes,
                used_on: used_on(image),
            }
        })
        .collect();

    let transitions: Vec<TransitionEntry> = plan
        .transitions
        .iter()
        .filter_map(|page| match page.transition {
            PageTransition::Effect { kind, duration_ms } => Some(TransitionEntry {
                page: page.index + 1,
                kind: transition_name(kind).into(),
                type_value: transition_value(kind),
                duration_ms,
            }),
            PageTransition::Global => None,
        })
        .collect();
    for t in &transitions {
        warnings.push(Warning {
            code: "transition_not_cut".into(),
            message: format!(
                "Page {} uses a {} transition ({}). On a slow PC transitions take longer than set; \
                 CUT avoids the delay.",
                t.page,
                transition_label(&t.kind),
                seconds(t.duration_ms)
            ),
            file: None,
        });
    }

    let images_shrunk = images.iter().filter(|i| i.action == Action::Shrunk).count();
    let images_renamed = images.iter().filter(|i| i.renamed_to.is_some()).count();
    let bytes_before = images.iter().map(|i| i.before.bytes).sum();
    let bytes_after = images
        .iter()
        .map(|i| i.after.as_ref().map_or(i.before.bytes, |a| a.bytes))
        .sum();
    let headline = headline(
        failure.is_some(),
        images.len(),
        images_shrunk,
        bytes_before,
        bytes_after,
        warnings.len(),
    );

    let mut report = Report {
        schema: SCHEMA,
        tool: ToolInfo {
            name: "hcg-optimizer".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        },
        bundle: BundleInfo {
            stem: plan.stem.clone(),
            hcg_path: plan.hcg_path.clone(),
            folder_path: plan.folder_path.clone(),
        },
        status: if failure.is_some() {
            Status::Failed
        } else {
            Status::Ok
        },
        summary: Summary {
            headline,
            images_total: images.len(),
            images_shrunk,
            images_renamed,
            bytes_before,
            bytes_after,
            warnings: warnings.len(),
        },
        failure,
        hcg: plan.hcg.as_ref().ok().map(|h| HcgInfo {
            version: h.version,
            pages: h.page_count,
            image_refs: h.image_refs,
        }),
        policy: PolicyInfo {
            max_width: policy.max_width,
            max_height: policy.max_height,
            max_bytes: policy.max_bytes,
            jpeg_quality: policy.jpeg_quality,
            min_jpeg_quality: policy.min_jpeg_quality,
        },
        images,
        missing,
        embedded,
        transitions,
        unreferenced_files: plan.unreferenced.len(),
        other_files: OtherFiles {
            unpaired: run.unpaired.clone(),
            ignored: run.ignored,
        },
        warnings,
        text: String::new(),
    };
    report.text = render_text(&report);
    report
}

/// The report as pretty-printed JSON.
pub fn to_json(report: &Report) -> String {
    // Serializing plain structs of strings and integers cannot fail.
    serde_json::to_string_pretty(report).unwrap_or_default() + "\n"
}

fn image_entry(
    file: &crate::analyze::FilePlan,
    done: Option<&FileResult>,
    warnings: &mut Vec<Warning>,
) -> ImageEntry {
    let path = file.refs.first().map(path_of).unwrap_or_default();
    let before = ImageInfo {
        format: file.probe.map_or("other", |p| format_name(p.format)).into(),
        width: file.probe.and_then(|p| p.dimensions).map(|d| d.0),
        height: file.probe.and_then(|p| p.dimensions).map(|d| d.1),
        bytes: file.bytes,
        quality: None,
    };
    let mut entry = ImageEntry {
        file: file.name.clone(),
        path,
        used_on: file.refs.iter().map(used_on).collect(),
        before,
        action: Action::Kept,
        reason: None,
        after: None,
        renamed_to: None,
    };
    let keep_reason = match (done, &file.action) {
        (Some(done), _) => match &done.outcome.action {
            ImageAction::Converted(c) => {
                entry.action = Action::Shrunk;
                let (format, quality) = match c.format {
                    OutputFormat::Jpeg { quality } => ("jpeg", Some(quality)),
                    OutputFormat::Png => ("png", None),
                };
                entry.after = Some(ImageInfo {
                    format: format.into(),
                    width: Some(c.width),
                    height: Some(c.height),
                    bytes: c.bytes.len() as u64,
                    quality,
                });
                if let Some(path) = &done.new_path {
                    entry.renamed_to = Some(Renamed {
                        file: done.new_name.clone(),
                        path: path.clone(),
                    });
                }
                None
            }
            ImageAction::Unchanged(reason) => Some(reason.clone()),
        },
        (None, PlannedAction::Keep(reason)) => Some(reason.clone()),
        // Planned but not processed: the bundle failed before reaching it.
        (None, PlannedAction::Shrink { .. }) => None,
    };
    let shown = entry
        .renamed_to
        .as_ref()
        .map_or(entry.file.clone(), |r| r.file.clone());
    if let Some(reason) = &keep_reason {
        entry.reason = Some(reason_code(reason).into());
        if let Some((code, message)) = reason_warning(reason, &shown) {
            warnings.push(Warning {
                code: code.into(),
                message,
                file: Some(entry.file.clone()),
            });
        }
    }
    let image_warnings = done
        .map(|d| d.outcome.warnings.as_slice())
        .unwrap_or_default();
    for ImageWarning::OverSizeLimit { bytes, limit } in image_warnings {
        warnings.push(Warning {
            code: "over_size_limit".into(),
            message: format!(
                "{shown} is still {} (limit {}){}.",
                human_size(*bytes),
                human_size(*limit),
                if entry.action == Action::Shrunk {
                    " even at the lowest JPEG quality"
                } else {
                    ""
                }
            ),
            file: Some(shown.clone()),
        });
    }
    entry
}

fn failure_code_and_advice(failure: &ApplyFailure) -> (&'static str, &'static str) {
    match failure {
        ApplyFailure::Parse(e) if matches!(e.kind, ParseErrorKind::UnsupportedVersion(_)) => (
            "unsupported_version",
            "Open the file in ez-CGER and save it again (the current version saves format 0x78), \
             then try again.",
        ),
        ApplyFailure::Parse(_) => (
            "parse_failed",
            "The file may be damaged. Open it in ez-CGER, save it again, and try again.",
        ),
        ApplyFailure::Edit(_) | ApplyFailure::Verify(_) => (
            "apply_failed",
            "Nothing was changed. This is a bug in hcg-optimizer; please report it with the file.",
        ),
    }
}

fn reason_code(reason: &KeepReason) -> &'static str {
    match reason {
        KeepReason::WithinLimits => "within_limits",
        KeepReason::NotResized(_) => "not_resized",
        KeepReason::NotSupportedByApp(_) => "not_supported_by_app",
        KeepReason::Undecodable(_) => "undecodable",
        KeepReason::TooManyPixels { .. } => "too_many_pixels",
        KeepReason::EncodeFailed(_) => "encode_failed",
        KeepReason::NoGain => "no_gain",
    }
}

fn reason_text(reason: &str) -> &'static str {
    match reason {
        "within_limits" => "already within limits",
        "not_resized" => "format left as is",
        "not_supported_by_app" => "format not supported by ez-CGER",
        "undecodable" => "could not be read",
        "too_many_pixels" => "too large to process",
        "encode_failed" => "could not be saved",
        "no_gain" => "re-saving would not make it smaller",
        _ => "kept",
    }
}

fn reason_warning(reason: &KeepReason, file: &str) -> Option<(&'static str, String)> {
    match reason {
        KeepReason::NotSupportedByApp(format) => Some((
            "not_supported_by_app",
            format!(
                "{file} is a {} image, which ez-CGER does not support.",
                format_label(*format)
            ),
        )),
        KeepReason::Undecodable(detail) => Some((
            "undecodable",
            format!("{file} could not be read as an image ({detail})."),
        )),
        KeepReason::TooManyPixels { width, height, .. } => Some((
            "too_many_pixels",
            format!("{file} ({width}x{height}) is too large to process and was left as is."),
        )),
        KeepReason::EncodeFailed(detail) => Some((
            "encode_failed",
            format!("{file} could not be saved ({detail})."),
        )),
        _ => None,
    }
}

fn format_name(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Jpeg => "jpeg",
        ImageFormat::Png => "png",
        ImageFormat::Bmp => "bmp",
        ImageFormat::Gif => "gif",
        ImageFormat::Dds => "dds",
        ImageFormat::Tga => "tga",
        ImageFormat::WebP => "webp",
        ImageFormat::Heic => "heic",
        ImageFormat::Other => "other",
    }
}

fn format_label(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::WebP => "WebP",
        ImageFormat::Heic => "HEIC",
        ImageFormat::Other => "unknown",
        other => format_name(other),
    }
}

fn transition_name(kind: TransitionKind) -> &'static str {
    match kind {
        TransitionKind::Cut => "cut",
        TransitionKind::Mix => "mix",
        TransitionKind::LeftToRight => "left_to_right",
        TransitionKind::RightToLeft => "right_to_left",
        TransitionKind::BottomToTop => "bottom_to_top",
        TransitionKind::TopToBottom => "top_to_bottom",
        TransitionKind::Unknown(_) => "unknown",
    }
}

fn transition_value(kind: TransitionKind) -> i32 {
    match kind {
        TransitionKind::Cut => 0,
        TransitionKind::Mix => 1,
        TransitionKind::LeftToRight => 2,
        TransitionKind::RightToLeft => 3,
        TransitionKind::BottomToTop => 4,
        TransitionKind::TopToBottom => 5,
        TransitionKind::Unknown(n) => n,
    }
}

fn transition_label(kind: &str) -> &'static str {
    match kind {
        "mix" => "Mix",
        "left_to_right" => "left-to-right",
        "right_to_left" => "right-to-left",
        "bottom_to_top" => "bottom-to-top",
        "top_to_bottom" => "top-to-bottom",
        "cut" => "CUT",
        _ => "unknown",
    }
}

fn path_of(image: &ImageRef) -> String {
    match &image.source {
        ImageSource::Path(p) => p.clone(),
        _ => String::new(),
    }
}

fn used_on(image: &ImageRef) -> UsedOn {
    let (role, frame) = match image.role {
        ImageRole::Primary => (Role::Primary, None),
        ImageRole::Sequence(i) => (Role::Sequence, Some(i + 1)),
    };
    UsedOn {
        page: image.page.map(|p| p + 1),
        layer: image.layer + 1,
        object: image.object + 1,
        role,
        frame,
    }
}

fn place(u: &UsedOn) -> String {
    let frame = u.frame.map(|f| format!(" frame {f}")).unwrap_or_default();
    match u.page {
        Some(page) => format!("page {page}, object {}{frame}", u.object),
        None => format!("background, object {}{frame}", u.object),
    }
}

fn places(used: &[UsedOn]) -> String {
    used.iter().map(place).collect::<Vec<_>>().join("; ")
}

/// Human-readable size: B below 1 KB, KB below 1 MB, else MB (binary units, one decimal).
fn human_size(bytes: u64) -> String {
    const KB: u64 = 1 << 10;
    const MB: u64 = 1 << 20;
    let one_decimal = |unit: u64, label: &str| {
        let tenths = (bytes * 10 + unit / 2) / unit;
        format!("{}.{} {label}", tenths / 10, tenths % 10)
    };
    match bytes {
        b if b < KB => format!("{b} B"),
        b if b < MB => one_decimal(KB, "KB"),
        _ => one_decimal(MB, "MB"),
    }
}

/// Upper-cases the first letter of a message.
fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn seconds(ms: i32) -> String {
    format!("{}.{} s", ms / 1000, (ms % 1000) / 100)
}

fn headline(
    failed: bool,
    total: usize,
    shrunk: usize,
    before: u64,
    after: u64,
    warnings: usize,
) -> String {
    let warn = match warnings {
        0 => "No warnings.".to_string(),
        1 => "1 warning.".to_string(),
        n => format!("{n} warnings."),
    };
    if failed {
        return format!("Not optimized: the file could not be processed. {warn}");
    }
    let noun = if total == 1 { "image" } else { "images" };
    if shrunk == 0 {
        return format!("Nothing to shrink: {total} {noun} already fine. {warn}");
    }
    format!(
        "{shrunk} of {total} {noun} shrunk: {} -> {}. {warn}",
        human_size(before),
        human_size(after)
    )
}

/// Plain-text rendering of a report (everything except `text` itself).
pub fn render_text(r: &Report) -> String {
    let mut out = Vec::new();
    let title = format!("hcg-optimizer report: {}", r.bundle.hcg_path);
    out.push(title.clone());
    out.push("=".repeat(title.chars().count()));
    out.push(r.summary.headline.clone());

    if let Some(f) = &r.failure {
        out.push(String::new());
        out.push(format!("FAILED: {}", f.message));
        out.push(format!("What to do: {}", f.advice));
    }
    if let Some(h) = &r.hcg {
        out.push(format!(
            "Format version 0x{:X}, {} page(s), {} image reference(s).",
            h.version, h.pages, h.image_refs
        ));
    }

    if !r.images.is_empty() {
        out.push(String::new());
        out.push("Images".into());
        for i in &r.images {
            let before = image_line(&i.before);
            let line = match (&i.after, &i.reason) {
                (Some(after), _) => format!("{before} -> {}", image_line(after)),
                (None, Some(reason)) => format!("{before}, kept ({})", reason_text(reason)),
                (None, None) => format!("{before}, not processed"),
            };
            out.push(format!("  {}", i.file));
            out.push(format!("    {line}"));
            if let Some(renamed) = &i.renamed_to {
                out.push(format!(
                    "    renamed to {} (path {})",
                    renamed.file, renamed.path
                ));
            }
            out.push(format!("    used on {}", places(&i.used_on)));
        }
    }

    if !r.missing.is_empty() {
        out.push(String::new());
        out.push("Missing from the image folder".into());
        for m in &r.missing {
            out.push(format!("  {} (used on {})", m.path, places(&m.used_on)));
        }
    }
    if !r.embedded.is_empty() {
        out.push(String::new());
        out.push("Embedded images (left as is)".into());
        for e in &r.embedded {
            let what = if e.kind == "embedded" {
                format!("embedded image, {}", human_size(e.bytes))
            } else {
                "empty image block".into()
            };
            out.push(format!("  {what} (used on {})", place(&e.used_on)));
        }
    }
    if !r.transitions.is_empty() {
        out.push(String::new());
        out.push("Page transitions other than CUT".into());
        for t in &r.transitions {
            out.push(format!(
                "  page {}: {}, {}",
                t.page,
                transition_label(&t.kind),
                seconds(t.duration_ms)
            ));
        }
    }
    if r.unreferenced_files > 0 || !r.other_files.unpaired.is_empty() {
        out.push(String::new());
        out.push("Other files".into());
        if r.unreferenced_files > 0 {
            out.push(format!(
                "  {} file(s) in the image folder are not used by the hcg (copied unchanged).",
                r.unreferenced_files
            ));
        }
        for f in &r.other_files.unpaired {
            out.push(format!(
                "  {f}: in the zip but not part of any hcg (not included)."
            ));
        }
    }

    out.push(String::new());
    if r.warnings.is_empty() {
        out.push("Warnings: none".into());
    } else {
        out.push("Warnings".into());
        for w in &r.warnings {
            out.push(format!("  ! {}", w.message));
        }
    }
    out.push(String::new());
    out.push(format!(
        "Limits: {}x{}, {}, JPEG quality {} down to {}.",
        r.policy.max_width,
        r.policy.max_height,
        human_size(r.policy.max_bytes),
        r.policy.jpeg_quality,
        r.policy.min_jpeg_quality
    ));
    out.join("\n") + "\n"
}

fn image_line(i: &ImageInfo) -> String {
    let size = match (i.width, i.height) {
        (Some(w), Some(h)) => format!("{w}x{h} "),
        _ => String::new(),
    };
    let quality = i.quality.map(|q| format!(" q{q}")).unwrap_or_default();
    format!(
        "{size}{}{quality} {}",
        i.format.to_uppercase(),
        human_size(i.bytes)
    )
}
