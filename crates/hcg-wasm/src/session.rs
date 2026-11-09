//! The browser session as plain Rust (issue #17), so it is tested natively. `lib.rs` wraps it
//! for JavaScript. It only calls the same `hcg-app` / adapter functions as the CLI.

use hcg_adapters::archive::{ZipLimits, read_zip, write_zip};
use hcg_adapters::image_codec::PureRustCodec;
use hcg_adapters::pdf::write_overview_pdf;
use hcg_app::overview::{OverviewSettings, PerSheet, page_overview};
use hcg_app::package::{InputFile, NameEncoding};
use hcg_app::policy::ResizePolicy;
use hcg_app::ports::Progress;
use hcg_app::report::plan_report;
use hcg_app::run::{RunPlan, execute, plan_run};
use serde::Deserialize;

/// Settings from the page's Advanced panel; every field is optional.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    max_width: Option<u32>,
    max_height: Option<u32>,
    max_bytes: Option<u64>,
    jpeg_quality: Option<u8>,
    min_jpeg_quality: Option<u8>,
}

/// Parses and validates the settings JSON (`""` or `{}` gives the defaults). Contract W8.
pub fn policy_from_json(json: &str) -> Result<ResizePolicy, String> {
    let settings: Settings = if json.trim().is_empty() {
        Settings::default()
    } else {
        serde_json::from_str(json).map_err(|e| format!("Invalid settings: {e}"))?
    };
    let mut policy = ResizePolicy::default();
    if let Some(v) = settings.max_width {
        policy.max_width = v;
    }
    if let Some(v) = settings.max_height {
        policy.max_height = v;
    }
    if let Some(v) = settings.max_bytes {
        policy.max_bytes = v;
    }
    if let Some(v) = settings.jpeg_quality {
        policy.jpeg_quality = v;
    }
    if let Some(v) = settings.min_jpeg_quality {
        policy.min_jpeg_quality = v;
    }
    policy
        .validate()
        .map_err(|e| format!("Invalid settings: {e}"))?;
    Ok(policy)
}

/// Overview options from the page; every field is optional.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct OverviewJson {
    per_sheet: Option<usize>,
}

/// Parses the overview settings JSON (`""` or `{}` gives the defaults).
pub fn overview_settings_from_json(json: &str) -> Result<OverviewSettings, String> {
    let parsed: OverviewJson = if json.trim().is_empty() {
        OverviewJson::default()
    } else {
        serde_json::from_str(json).map_err(|e| format!("Invalid settings: {e}"))?
    };
    let per_sheet = match parsed.per_sheet {
        None => PerSheet::default(),
        Some(n) => PerSheet::from_count(n)
            .ok_or_else(|| format!("Invalid settings: per_sheet must be 6, 12 or 20, got {n}"))?,
    };
    Ok(OverviewSettings { per_sheet })
}

/// One zip (or a bare .hcg): read and planned on creation, optimized once.
pub struct SessionCore {
    policy: ResizePolicy,
    codec: PureRustCodec,
    plan: Option<RunPlan>,
    plan_json: String,
    reports_json: String,
}

impl SessionCore {
    /// Reads the zip (then frees it) and plans the run. Decodes no image (contract W4).
    pub fn new(zip: Vec<u8>, settings_json: &str) -> Result<Self, String> {
        let policy = policy_from_json(settings_json)?;
        let files = read_zip(&zip, &ZipLimits::default())
            .map_err(|e| format!("This file could not be opened as a zip: {e}"))?;
        drop(zip);
        Ok(Self::plan(files, policy))
    }

    /// Like [`SessionCore::new`], but a file named `*.hcg` is taken as a bare hcg without its
    /// image folder (enough for the page overview; the plan reports the missing images).
    pub fn open(bytes: Vec<u8>, file_name: &str, settings_json: &str) -> Result<Self, String> {
        if !file_name.to_ascii_lowercase().ends_with(".hcg") {
            return Self::new(bytes, settings_json);
        }
        let policy = policy_from_json(settings_json)?;
        let file = InputFile {
            path: file_name.to_string(),
            bytes,
            name_encoding: NameEncoding::Utf8Guessed,
        };
        Ok(Self::plan(vec![file], policy))
    }

    fn plan(files: Vec<InputFile>, policy: ResizePolicy) -> Self {
        let codec = PureRustCodec::default();
        let plan = plan_run(files, &codec, &policy);
        let plan_json = serde_json::to_string(&plan_report(&plan)).unwrap_or_default();
        Self {
            policy,
            codec,
            plan: Some(plan),
            plan_json,
            reports_json: "[]".into(),
        }
    }

    /// The preview (`hcg_app::report::PlanReport`) as JSON.
    pub fn plan_json(&self) -> &str {
        &self.plan_json
    }

    /// The page overview PDF of bundle `index` (order of the plan's `bundles`). Available until
    /// `optimize` runs.
    pub fn overview_pdf(&self, index: usize, settings_json: &str) -> Result<Vec<u8>, String> {
        let settings = overview_settings_from_json(settings_json)?;
        let plan = self
            .plan
            .as_ref()
            .ok_or("Open the file again to make the page overview.")?;
        let (bundle, _) = plan
            .bundles
            .get(index)
            .ok_or_else(|| format!("There is no .hcg number {}.", index + 1))?;
        let overview = page_overview(bundle, &settings)
            .map_err(|e| format!("{} could not be read: {e}", bundle.hcg_path))?;
        Ok(write_overview_pdf(&overview).bytes)
    }

    /// Shrinks images and returns the output zip. Can run once per session.
    pub fn optimize(&mut self, progress: &mut impl Progress) -> Result<Vec<u8>, String> {
        let plan = self.plan.take().ok_or("This zip was already optimized.")?;
        if plan.bundles.is_empty() {
            return Err("No .hcg file was found in the zip.".into());
        }
        let result = execute(plan, &self.codec, &self.policy, progress)
            .map_err(|_| "Cancelled.".to_string())?;
        self.reports_json =
            serde_json::to_string(&result.reports(&self.policy)).unwrap_or_default();
        write_zip(result.package(&self.policy))
            .map_err(|e| format!("The output zip could not be written: {e}"))
    }

    /// The reports (`hcg_app::report::Report[]`) as JSON; `[]` before `optimize`.
    pub fn reports_json(&self) -> &str {
        &self.reports_json
    }
}
