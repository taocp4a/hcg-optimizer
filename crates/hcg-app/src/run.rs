//! run: the use case frontends call (issue #9). Input files -> bundles -> plans -> results.
//! Split into [`plan_run`] (cheap, for a preview or `--dry-run`) and [`execute`] (heavy).

use crate::analyze::{BundlePlan, analyze};
use crate::apply::{BundleResult, Cancelled, apply};
use crate::bundle::Bundle;
use crate::package::{InputFile, OutputFile, PackagedBundle, output_files, pair_bundles};
use crate::policy::ResizePolicy;
use crate::ports::{ImageCodec, Progress};
use crate::report::{Report, RunNotes, build_report};

/// Plans for every bundle in the input, plus files that belong to no bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunPlan {
    pub bundles: Vec<(Bundle, BundlePlan)>,
    /// Files that belong to no hcg, sorted.
    pub unpaired: Vec<String>,
    /// OS clutter dropped from the input, sorted.
    pub ignored: Vec<String>,
}

impl RunPlan {
    /// Number of files that will be decoded and re-encoded.
    pub fn shrink_count(&self) -> usize {
        self.bundles.iter().map(|(_, p)| p.shrink_count()).sum()
    }
}

/// Results for every bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunResult {
    pub bundles: Vec<BundleResult>,
    pub unpaired: Vec<String>,
    pub ignored: Vec<String>,
}

impl RunResult {
    /// Structured reports, one per bundle, for a viewer (see [`crate::report`]).
    pub fn reports(&self, policy: &ResizePolicy) -> Vec<Report> {
        let notes = self.notes();
        self.bundles
            .iter()
            .map(|b| build_report(b, &notes, policy))
            .collect()
    }

    /// The output package with each bundle's report rendered as `report.txt`.
    pub fn package(&self, policy: &ResizePolicy) -> Vec<OutputFile> {
        let notes = self.notes();
        self.output_files(|bundle| build_report(bundle, &notes, policy).text)
    }

    /// Zip leftovers, as the report shows them.
    pub fn notes(&self) -> RunNotes {
        RunNotes {
            unpaired: self.unpaired.clone(),
            ignored: self.ignored.len(),
        }
    }

    /// The output package: per bundle `<stem>_optimized/` with the optimized files (or only the
    /// report if the bundle failed) and `report.txt` with the text `report` returns.
    pub fn output_files(&self, report: impl Fn(&BundleResult) -> String) -> Vec<OutputFile> {
        let reports: Vec<String> = self.bundles.iter().map(&report).collect();
        let packaged: Vec<PackagedBundle<'_>> = self
            .bundles
            .iter()
            .zip(&reports)
            .map(|(b, r)| PackagedBundle {
                stem: &b.plan.stem,
                bundle: b.output.as_ref(),
                report: r,
            })
            .collect();
        output_files(&packaged)
    }
}

/// Groups the input into bundles and plans each one. Decodes nothing.
pub fn plan_run<C: ImageCodec>(files: Vec<InputFile>, codec: &C, policy: &ResizePolicy) -> RunPlan {
    let pairing = pair_bundles(files);
    let bundles = pairing
        .bundles
        .into_iter()
        .map(|bundle| {
            let plan = analyze(codec, &bundle, policy);
            (bundle, plan)
        })
        .collect();
    RunPlan {
        bundles,
        unpaired: pairing.unpaired,
        ignored: pairing.ignored,
    }
}

/// Applies every plan. Progress counts files to shrink across all bundles.
pub fn execute<C: ImageCodec, P: Progress>(
    plan: RunPlan,
    codec: &C,
    policy: &ResizePolicy,
    progress: &mut P,
) -> Result<RunResult, Cancelled> {
    let total = plan.shrink_count();
    let mut offset = 0;
    let mut bundles = Vec::with_capacity(plan.bundles.len());
    for (bundle, bundle_plan) in plan.bundles {
        let count = bundle_plan.shrink_count();
        let mut scoped = Scoped {
            inner: progress,
            offset,
            total,
        };
        bundles.push(apply(codec, &bundle, bundle_plan, policy, &mut scoped)?);
        offset += count;
    }
    Ok(RunResult {
        bundles,
        unpaired: plan.unpaired,
        ignored: plan.ignored,
    })
}

/// [`plan_run`] followed by [`execute`].
pub fn run<C: ImageCodec, P: Progress>(
    files: Vec<InputFile>,
    codec: &C,
    policy: &ResizePolicy,
    progress: &mut P,
) -> Result<RunResult, Cancelled> {
    execute(plan_run(files, codec, policy), codec, policy, progress)
}

/// Maps one bundle's progress onto the whole run.
struct Scoped<'a, P> {
    inner: &'a mut P,
    offset: usize,
    total: usize,
}

impl<P: Progress> Progress for Scoped<'_, P> {
    fn report(&mut self, done: usize, _total: usize, message: &str) {
        self.inner.report(self.offset + done, self.total, message);
    }

    fn is_cancelled(&self) -> bool {
        self.inner.is_cancelled()
    }
}
