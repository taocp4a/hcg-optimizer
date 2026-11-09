//! The browser session as plain Rust (issue #17), so it is tested natively. `lib.rs` wraps it
//! for JavaScript. It only calls the same `hcg-app` / adapter functions as the CLI.

use hcg_adapters::archive::{ZipLimits, read_zip, write_zip};
use hcg_adapters::image_codec::PureRustCodec;
use hcg_adapters::pdf::{Thumbnails, overview_pdf};
use hcg_app::overview::{OverviewSettings, PerSheet};
use hcg_app::package::{InputFile, NameEncoding};
use hcg_app::policy::ResizePolicy;
use hcg_app::ports::Progress;
use hcg_app::report::{Language, plan_report_in};
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
    /// Report language: `en` (default) or `ko` (the web page, issue #21).
    language: Option<String>,
}

/// Parses and validates the settings JSON (`""` or `{}` gives the defaults). Contract W8.
pub fn policy_from_json(json: &str) -> Result<ResizePolicy, String> {
    settings_from_json(json).map(|(policy, _)| policy)
}

/// The policy and the report language from the settings JSON.
pub fn settings_from_json(json: &str) -> Result<(ResizePolicy, Language), String> {
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
    let language = match settings.language.as_deref() {
        None => Language::En,
        Some(code) => Language::from_code(code)
            .ok_or_else(|| format!("Invalid settings: unknown language {code:?}"))?,
    };
    Ok((policy, language))
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

/// A message in the session's language. Settings errors stay English: the page always sends
/// valid settings, so only developers see them.
fn say(language: Language, en: String, ko: String) -> String {
    match language {
        Language::En => en,
        Language::Ko => ko,
    }
}

/// One zip (or a bare .hcg): read and planned on creation, optimized once.
pub struct SessionCore {
    policy: ResizePolicy,
    language: Language,
    codec: PureRustCodec,
    plan: Option<RunPlan>,
    /// Prepared page thumbnails per bundle (plan order), made on first use and before
    /// optimizing, so the overview still works after the bundles are gone (issue #21).
    thumbnails: Vec<Option<Thumbnails>>,
    plan_json: String,
    reports_json: String,
}

impl SessionCore {
    /// Reads the zip (then frees it) and plans the run. Decodes no image (contract W4).
    pub fn new(zip: Vec<u8>, settings_json: &str) -> Result<Self, String> {
        let (policy, language) = settings_from_json(settings_json)?;
        let files = read_zip(&zip, &ZipLimits::default())
            .map_err(|e| {
                say(
                    language,
                    format!("This file could not be opened as a zip: {e}"),
                    format!("zip 파일을 열 수 없습니다. 압축 파일이 손상되지 않았는지 확인하십시오. (기술 정보: {e})"),
                )
            })?;
        drop(zip);
        Ok(Self::plan(files, policy, language))
    }

    /// Like [`SessionCore::new`], but a file named `*.hcg` is taken as a bare hcg without its
    /// image folder (enough for the page overview; the plan reports the missing images).
    pub fn open(bytes: Vec<u8>, file_name: &str, settings_json: &str) -> Result<Self, String> {
        if !file_name.to_ascii_lowercase().ends_with(".hcg") {
            return Self::new(bytes, settings_json);
        }
        let (policy, language) = settings_from_json(settings_json)?;
        let file = InputFile {
            path: file_name.to_string(),
            bytes,
            name_encoding: NameEncoding::Utf8Guessed,
        };
        Ok(Self::plan(vec![file], policy, language))
    }

    fn plan(files: Vec<InputFile>, policy: ResizePolicy, language: Language) -> Self {
        let codec = PureRustCodec::default();
        let plan = plan_run(files, &codec, &policy);
        let plan_json = serde_json::to_string(&plan_report_in(&plan, language)).unwrap_or_default();
        Self {
            policy,
            language,
            codec,
            thumbnails: vec![None; plan.bundles.len()],
            plan: Some(plan),
            plan_json,
            reports_json: "[]".into(),
        }
    }

    /// The preview (`hcg_app::report::PlanReport`) as JSON.
    pub fn plan_json(&self) -> &str {
        &self.plan_json
    }

    /// The page overview PDF of bundle `index` (order of the plan's `bundles`), before or after
    /// `optimize`.
    pub fn overview_pdf(&mut self, index: usize, settings_json: &str) -> Result<Vec<u8>, String> {
        let settings = overview_settings_from_json(settings_json)?;
        self.prepare_thumbnails(index)?;
        let thumbnails = self.thumbnails[index]
            .as_ref()
            .ok_or_else(|| self.unavailable())?;
        Ok(overview_pdf(thumbnails, &settings).bytes)
    }

    fn unavailable(&self) -> String {
        say(
            self.language,
            "The page overview is not available for this file.".into(),
            "이 파일은 페이지 PDF를 만들 수 없습니다.".into(),
        )
    }

    /// Prepares bundle `index`'s thumbnails from the plan if not done yet.
    fn prepare_thumbnails(&mut self, index: usize) -> Result<(), String> {
        let slot = self
            .thumbnails
            .get(index)
            .ok_or_else(|| format!("There is no .hcg number {}.", index + 1))?;
        if slot.is_some() {
            return Ok(());
        }
        let plan = self.plan.as_ref().ok_or_else(|| self.unavailable())?;
        let (bundle, _) = &plan.bundles[index];
        let thumbnails = Thumbnails::from_bundle(bundle).map_err(|e| {
            say(
                self.language,
                format!("{} could not be read: {e}", bundle.hcg_path),
                format!(
                    "{} 파일을 읽을 수 없습니다. (기술 정보: {e})",
                    bundle.hcg_path
                ),
            )
        })?;
        self.thumbnails[index] = Some(thumbnails);
        Ok(())
    }

    /// Shrinks images and returns the output zip. Can run once per session.
    pub fn optimize(&mut self, progress: &mut impl Progress) -> Result<Vec<u8>, String> {
        let language = self.language;
        let plan = self.plan.take().ok_or_else(|| {
            say(
                language,
                "This zip was already optimized.".into(),
                "이미 최적화한 파일입니다. 파일을 다시 선택하십시오.".into(),
            )
        })?;
        if plan.bundles.is_empty() {
            return Err(say(
                language,
                "No .hcg file was found in the zip.".into(),
                "zip 파일 안에 .hcg 파일이 없습니다.".into(),
            ));
        }
        let result = execute(plan, &self.codec, &self.policy, progress)
            .map_err(|_| say(language, "Cancelled.".into(), "취소되었습니다.".into()))?;
        // Keep the page overview available: prepare thumbnails from the optimized hcg (same
        // thumbnails, only image paths differ) before the bundles move into the zip.
        for (slot, bundle) in self.thumbnails.iter_mut().zip(&result.bundles) {
            if slot.is_none()
                && let Some(output) = &bundle.output
            {
                *slot = Thumbnails::from_bundle(output).ok();
            }
        }
        self.reports_json = serde_json::to_string(&result.reports_in(&self.policy, self.language))
            .unwrap_or_default();
        write_zip(result.package_in(&self.policy, self.language)).map_err(|e| {
            say(
                language,
                format!("The output zip could not be written: {e}"),
                format!("결과 파일을 만들지 못했습니다. (기술 정보: {e})"),
            )
        })
    }

    /// The reports (`hcg_app::report::Report[]`) as JSON; `[]` before `optimize`.
    pub fn reports_json(&self) -> &str {
        &self.reports_json
    }
}
