//! `hcg-optimizer`: the command-line frontend (development and testing, issue #15).
//!
//! Frontend layer: arguments, file I/O, and display only. All processing goes through the same
//! `hcg-app` calls the web page uses: `read_zip` -> `plan_run` -> `execute` -> `package` ->
//! `write_zip`; with `--pdf`, `pair_bundles` -> `Thumbnails::from_bundle` -> `overview_pdf`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use hcg_adapters::archive::{ZipLimits, read_zip, write_zip};
use hcg_adapters::image_codec::PureRustCodec;
use hcg_adapters::pdf::{Thumbnails, overview_pdf};
use hcg_app::overview::{OverviewSettings, PerSheet, overview_file_name};
use hcg_app::package::{InputFile, NameEncoding, pair_bundles};
use hcg_app::policy::{KeepReason, PlannedAction, ResizePolicy};
use hcg_app::ports::{ImageFormat, Progress};
use hcg_app::run::{RunPlan, execute, plan_run};

/// Shrink large images in ez-CGER .hcg projects (development and testing CLI).
#[derive(Debug, Parser)]
#[command(name = "hcg-optimizer", version)]
struct Args {
    /// A zip with `xxx.hcg` and its `xxx/` image folder, or an .hcg file (its same-named
    /// sibling folder is added automatically).
    input: PathBuf,
    /// Output zip [default: <input name>_optimized.zip next to the input]; with --pdf, the PDF
    /// [default: <hcg name>_pages.pdf next to the input].
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Print the plan only: no decoding, nothing written.
    #[arg(long)]
    dry_run: bool,
    /// Print the reports as JSON (the viewer's data) instead of text.
    #[arg(long)]
    json: bool,
    /// Write a page overview PDF (page thumbnails, A4 landscape) instead of optimizing. No image
    /// folder needed.
    #[arg(long, conflicts_with_all = ["dry_run", "json"])]
    pdf: bool,
    /// Thumbnails per PDF sheet with --pdf: 6, 12 or 20 [default: 12].
    #[arg(long, value_name = "N", requires = "pdf")]
    per_sheet: Option<usize>,
    /// Overwrite an existing output file.
    #[arg(long)]
    force: bool,
    /// Largest output size, WIDTHxHEIGHT [default: 1920x1080].
    #[arg(long, value_name = "WxH")]
    max_size: Option<String>,
    /// Target file size, e.g. 1MiB, 800KB, 500000 [default: 1MiB].
    #[arg(long, value_name = "SIZE")]
    max_bytes: Option<String>,
    /// First JPEG quality tried [default: 90].
    #[arg(long, value_name = "Q")]
    quality: Option<u8>,
    /// Lowest JPEG quality tried [default: 50].
    #[arg(long, value_name = "Q")]
    min_quality: Option<u8>,
}

/// Exit codes (contract L6).
const EXIT_FAILED: u8 = 1;
const EXIT_USAGE: u8 = 2;
const EXIT_IO: u8 = 3;

struct Failure(u8, String);

fn main() -> ExitCode {
    match run(Args::parse()) {
        Ok(code) => ExitCode::from(code),
        Err(Failure(code, message)) => {
            eprintln!("hcg-optimizer: {message}");
            ExitCode::from(code)
        }
    }
}

fn run(args: Args) -> Result<u8, Failure> {
    let usage = |m: String| Failure(EXIT_USAGE, m);
    let policy = policy_from(&args).map_err(usage)?;
    policy
        .validate()
        .map_err(|e| usage(format!("invalid setting: {e}")))?;

    let input = fs::canonicalize(&args.input)
        .map_err(|e| Failure(EXIT_IO, format!("{}: {e}", args.input.display())))?;
    if args.pdf {
        return run_pdf(&args, &input);
    }
    let output = args
        .output
        .clone()
        .unwrap_or_else(|| default_output(&input));
    if !args.dry_run {
        check_output(&input, &output, args.force)?;
    }

    let codec = PureRustCodec::default();
    let files = read_input(&input)?;
    let plan = plan_run(files, &codec, &policy);
    if plan.bundles.is_empty() {
        return Err(Failure(
            EXIT_FAILED,
            format!("no .hcg file found in {}", input.display()),
        ));
    }
    if args.dry_run {
        print!("{}", describe_plan(&plan));
        return Ok(0);
    }

    let result = execute(plan, &codec, &policy, &mut StderrProgress)
        .map_err(|_| Failure(EXIT_FAILED, "cancelled".into()))?;
    let failed = result.bundles.iter().any(|b| b.failure.is_some());
    let reports = result.reports(&policy);
    let zip = write_zip(result.package(&policy))
        .map_err(|e| Failure(EXIT_IO, format!("writing the zip: {e}")))?;
    write_atomically(&output, &zip)?;

    let mut stdout = std::io::stdout().lock();
    let printed = if args.json {
        serde_json::to_string_pretty(&reports)
            .map(|json| json + "\n")
            .unwrap_or_default()
    } else {
        reports
            .iter()
            .map(|r| r.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    };
    let _ = stdout.write_all(printed.as_bytes());
    eprintln!(
        "Wrote {} ({})",
        output.display(),
        human_size(zip.len() as u64)
    );
    Ok(if failed { EXIT_FAILED } else { 0 })
}

/// `--pdf`: one page overview per hcg in the input (issue #19).
fn run_pdf(args: &Args, input: &Path) -> Result<u8, Failure> {
    let per_sheet = match args.per_sheet {
        None => PerSheet::default(),
        Some(n) => PerSheet::from_count(n).ok_or_else(|| {
            Failure(
                EXIT_USAGE,
                format!("--per-sheet must be 6, 12 or 20, got {n}"),
            )
        })?,
    };
    let settings = OverviewSettings { per_sheet };
    let bundles = pair_bundles(read_input(input)?).bundles;
    if bundles.is_empty() {
        return Err(Failure(
            EXIT_FAILED,
            format!("no .hcg file found in {}", input.display()),
        ));
    }
    if args.output.is_some() && bundles.len() > 1 {
        return Err(Failure(
            EXIT_USAGE,
            format!(
                "-o names one PDF, but the input has {} .hcg files; leave it out",
                bundles.len()
            ),
        ));
    }
    let mut failed = false;
    for bundle in &bundles {
        let thumbnails = match Thumbnails::from_bundle(bundle) {
            Ok(thumbnails) => thumbnails,
            Err(e) => {
                eprintln!("{}: cannot be read: {e}", bundle.hcg_path);
                failed = true;
                continue;
            }
        };
        let output = args
            .output
            .clone()
            .unwrap_or_else(|| input.with_file_name(overview_file_name(&bundle.stem)));
        check_output(input, &output, args.force)?;
        let pdf = overview_pdf(&thumbnails, &settings);
        write_atomically(&output, &pdf.bytes)?;
        eprintln!(
            "Wrote {} ({} page(s) on {} sheet(s), {})",
            output.display(),
            pdf.page_count,
            pdf.sheets,
            human_size(pdf.bytes.len() as u64)
        );
        if !pdf.blank_pages.is_empty() {
            let pages: Vec<String> = pdf.blank_pages.iter().map(|p| p.to_string()).collect();
            eprintln!("  no thumbnail for page(s) {}", pages.join(", "));
        }
    }
    Ok(if failed { EXIT_FAILED } else { 0 })
}

fn policy_from(args: &Args) -> Result<ResizePolicy, String> {
    let mut policy = ResizePolicy::default();
    if let Some(size) = &args.max_size {
        let parsed = size
            .split_once(['x', 'X'])
            .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)));
        let (w, h) =
            parsed.ok_or_else(|| format!("--max-size must look like 1920x1080, got {size:?}"))?;
        policy.max_width = w;
        policy.max_height = h;
    }
    if let Some(bytes) = &args.max_bytes {
        policy.max_bytes =
            parse_size(bytes).ok_or_else(|| format!("--max-bytes: cannot read {bytes:?}"))?;
    }
    if let Some(q) = args.quality {
        policy.jpeg_quality = q;
    }
    if let Some(q) = args.min_quality {
        policy.min_jpeg_quality = q;
    }
    Ok(policy)
}

/// `1MiB`, `1MB` (both 1,048,576), `800KB`, `500000`, `2 MiB`.
fn parse_size(text: &str) -> Option<u64> {
    let text = text.trim();
    let split = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let number: u64 = number.parse().ok()?;
    let factor = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "k" | "kb" | "kib" => 1 << 10,
        "m" | "mb" | "mib" => 1 << 20,
        _ => return None,
    };
    number.checked_mul(factor)
}

fn default_output(input: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .map_or_else(|| "output".into(), |s| s.to_string_lossy().into_owned());
    input.with_file_name(format!("{stem}_optimized.zip"))
}

/// Contract L1: never the input, never an existing file without --force.
fn check_output(input: &Path, output: &Path, force: bool) -> Result<(), Failure> {
    let same = fs::canonicalize(output).is_ok_and(|o| o == input);
    if same {
        return Err(Failure(
            EXIT_USAGE,
            "the output would overwrite the input".into(),
        ));
    }
    if output.exists() && !force {
        return Err(Failure(
            EXIT_USAGE,
            format!(
                "{} already exists (use --force to overwrite)",
                output.display()
            ),
        ));
    }
    Ok(())
}

/// Contract L2: write next to the output, then rename; remove the temporary file on failure.
fn write_atomically(output: &Path, bytes: &[u8]) -> Result<(), Failure> {
    let name = output
        .file_name()
        .map_or_else(|| "output".into(), |n| n.to_string_lossy().into_owned());
    let partial = output.with_file_name(format!(".{name}.partial"));
    let result = fs::write(&partial, bytes).and_then(|()| fs::rename(&partial, output));
    result.map_err(|e| {
        let _ = fs::remove_file(&partial);
        Failure(EXIT_IO, format!("{}: {e}", output.display()))
    })
}

/// A zip is read as is; an .hcg brings its same-named sibling folder.
fn read_input(path: &Path) -> Result<Vec<InputFile>, Failure> {
    let io = |p: &Path, e: std::io::Error| Failure(EXIT_IO, format!("{}: {e}", p.display()));
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "zip" => {
            let bytes = fs::read(path).map_err(|e| io(path, e))?;
            read_zip(&bytes, &ZipLimits::default())
                .map_err(|e| Failure(EXIT_FAILED, format!("{}: {e}", path.display())))
        }
        "hcg" => {
            let file_name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut files = vec![InputFile {
                path: file_name,
                bytes: fs::read(path).map_err(|e| io(path, e))?,
                name_encoding: NameEncoding::Utf8Guessed,
            }];
            let folder = path.with_extension("");
            if folder.is_dir() {
                let stem = folder
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let mut entries: Vec<_> = fs::read_dir(&folder)
                    .map_err(|e| io(&folder, e))?
                    .filter_map(Result::ok)
                    .filter(|e| e.path().is_file())
                    .collect();
                entries.sort_by_key(|e| e.file_name());
                for entry in entries {
                    files.push(InputFile {
                        path: format!("{stem}/{}", entry.file_name().to_string_lossy()),
                        bytes: fs::read(entry.path()).map_err(|e| io(&entry.path(), e))?,
                        name_encoding: NameEncoding::Utf8Guessed,
                    });
                }
            }
            Ok(files)
        }
        _ => Err(Failure(
            EXIT_USAGE,
            format!("{}: expected a .zip or .hcg file", path.display()),
        )),
    }
}

/// Plain-text summary of a plan for --dry-run.
fn describe_plan(plan: &RunPlan) -> String {
    let mut out = String::new();
    for (_, p) in &plan.bundles {
        out += &format!("{}\n", p.hcg_path);
        if let Err(e) = &p.hcg {
            out += &format!("  cannot be processed: {e}\n");
            continue;
        }
        out += &format!(
            "  {} of {} referenced file(s) will be shrunk\n",
            p.shrink_count(),
            p.files.len()
        );
        for f in &p.files {
            let size = f
                .probe
                .and_then(|p| p.dimensions)
                .map(|(w, h)| format!("{w}x{h} "))
                .unwrap_or_default();
            let format = f.probe.map_or("?", |p| format_name(p.format));
            let action = match &f.action {
                PlannedAction::Shrink { target: (w, h) } => format!("shrink to fit {w}x{h}"),
                PlannedAction::Keep(reason) => format!("keep ({})", keep_text(reason)),
            };
            out += &format!(
                "  {}  {size}{format} {}  -> {action}\n",
                f.name,
                human_size(f.bytes)
            );
        }
        if !p.missing.is_empty() {
            out += &format!(
                "  {} reference(s) missing from the image folder\n",
                p.missing.len()
            );
        }
        if !p.transitions.is_empty() {
            out += &format!(
                "  {} page(s) with a transition other than CUT\n",
                p.transitions.len()
            );
        }
    }
    for f in &plan.unpaired {
        out += &format!("not part of any hcg: {f}\n");
    }
    out
}

fn keep_text(reason: &KeepReason) -> &'static str {
    match reason {
        KeepReason::WithinLimits => "already within limits",
        KeepReason::NotResized(_) => "format left as is",
        KeepReason::NotSupportedByApp(_) => "format not supported by ez-CGER",
        KeepReason::Undecodable(_) => "could not be read",
        KeepReason::TooManyPixels { .. } => "too large to process",
        KeepReason::EncodeFailed(_) => "could not be saved",
        KeepReason::NoGain => "would not get smaller",
    }
}

fn format_name(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Jpeg => "JPEG",
        ImageFormat::Png => "PNG",
        ImageFormat::Bmp => "BMP",
        ImageFormat::Gif => "GIF",
        ImageFormat::Dds => "DDS",
        ImageFormat::Tga => "TGA",
        ImageFormat::WebP => "WebP",
        ImageFormat::Heic => "HEIC",
        ImageFormat::Other => "unknown",
    }
}

fn human_size(bytes: u64) -> String {
    match bytes {
        b if b < 1 << 10 => format!("{b} B"),
        b if b < 1 << 20 => format!("{:.1} KB", b as f64 / 1024.0),
        b => format!("{:.1} MB", b as f64 / 1_048_576.0),
    }
}

/// Progress on stderr: one line per image.
struct StderrProgress;

impl Progress for StderrProgress {
    fn report(&mut self, done: usize, total: usize, message: &str) {
        if !message.is_empty() {
            eprintln!("[{}/{total}] {message}", done + 1);
        }
    }
}
