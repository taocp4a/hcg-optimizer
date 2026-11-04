# Architecture

The project follows clean architecture. **Dependencies point inward only**, and inner layers know nothing about outer ones.
Changing or adding a frontend (web, CLI, a future GUI, ...) therefore leaves domain and use-case code untouched.

Current frontends: **WASM (web, for users)** and **CLI (development and testing)**. A GUI is not planned, but adding one only requires a new frontend crate.

## Layers

```
┌─ Frontends ─────────────────────────────────────────────────────────┐
│  hcg-wasm + web/ (browser)          hcg-cli (terminal)     [hcg-gui …] │
│  I/O and presentation only. Picks port implementations and hands     │
│  them to the use cases                                                │
└───────────────┬───────────────────────────────┬─────────────────────┘
                │                               │
                ▼                               ▼
┌─ Adapters ──────────────────────────────────────────────────────────┐
│  hcg-adapters: zip <-> file list (UTF-8/CP949), pure-Rust ImageCodec  │
│  (a web-specific implementation, if needed, lives in hcg-wasm and     │
│  implements the same trait)                                           │
└───────────────────────────────┬─────────────────────────────────────┘
                                │ implements ports (traits)
                                ▼
┌─ Application ───────────────────────────────────────────────────────┐
│  hcg-app: analyze(Bundle) -> Plan/Report, apply(Bundle, Plan) -> Bundle│
│  resize policy (1920×1080, 1 MB, alpha detection), report             │
│  ports: ImageCodec, Progress  ·  unit of work: Bundle                 │
└───────────────────────────────┬─────────────────────────────────────┘
                                ▼
┌─ Domain ────────────────────────────────────────────────────────────┐
│  hcg-format: hcg model, parser/writer, image ID (pure, no I/O)        │
└─────────────────────────────────────────────────────────────────────┘
```

| Crate | Layer | Responsibility | May depend on |
|---|---|---|---|
| `hcg-format` | domain | hcg bytes <-> model, image ID | pure crates only (crc32fast, ...) |
| `hcg-app` | application | analyze/apply, policy, report, port definitions | hcg-format |
| `hcg-adapters` | adapters | zip <-> file list, ImageCodec implementation | hcg-app, hcg-format, zip/image/encoding_rs |
| `hcg-cli` | frontend | arguments, file I/O, terminal progress | all of the above |
| `hcg-wasm` | frontend | JS <-> Rust bytes, `JsError` conversion, on-page progress | all of the above + wasm-bindgen |

**`project/scripts/check_layers.py` enforces these rules** (preflight step 2):
- Every crate must be assigned a layer.
- No dependency on the same layer or an outer layer.
- Domain and application must not depend on I/O or platform crates (zip, image, wasm-bindgen, ...).

## Why the boundaries are drawn here

- **analyze and apply are separate.** First build a plan and report (changing nothing), then apply it. The web "this is what will be shrunk" confirmation, a CLI `--dry-run`, and a possible future GUI preview all share this code.
- **ImageCodec is a port (trait), split into steps** (`probe`, `decode_fit`, `uses_alpha`, `dominant_border_color`, `encode`). A photo is decoded and downscaled once; only the small result is re-encoded while the policy searches for a JPEG quality. Images within the limits are skipped after reading only the header. The default implementation uses the pure-Rust `image` crate; the web frontend can swap in the browser's encoder if needed.
- **Progress is a port.** Each frontend shows progress and cancellation its own way while the use-case code stays single.
- **zip stays outside the use cases.** The adapter only turns a zip into a flat file list and back. Grouping files into bundles (`pair_bundles`) and the output layout (`output_files`) are use-case rules in `hcg-app::package`, testable without zip. A different input form (e.g. picking a folder directly) only needs a new adapter.
- **hcg-format has no traits.** There is only one format, so it is concrete code. No needless abstraction.

## Module plan

```
hcg-format/src/          # ✅ (issue #3)
├── lib.rs           # public surface re-exports
├── error.rs         # ParseError (offset + context path), EditError
├── reader.rs        # offset-tracking reader; checks remaining length before every read
├── model.rs         # HcgFile / Page / Layer / Object / SafeBmp; unchanged data kept raw
├── parse.rs         # §1–§6 in order, version 0x78 only
├── write.rs         # infallible writer, same order
├── edit.rs          # pages(), images(), set_image_path (the only mutation)
└── image_id.rs      # §6.3 CRC32 ID

hcg-app/src/
├── bundle.rs        # ✅ Bundle / BundleFile
├── package.rs       # ✅ InputFile/OutputFile, pair_bundles, output_files (issue #5)
├── ports.rs         # ✅ ImageCodec (probe/decode_fit/encode), Progress, NoProgress
├── policy.rs        # ✅ ResizePolicy, optimize_image, fit_within, output_file_name (issue #7)
├── analyze.rs       # Bundle -> Plan (image matching, missing files, transition and version warnings)
├── apply.rs         # execute the Plan: convert images, update paths/IDs, rewrite hcg, re-parse check
└── report.rs        # Plan/result -> report.txt (or html)

hcg-adapters/src/
├── archive.rs       # ✅ read_zip / write_zip, UTF-8/CP949 names, zip-bomb limits (issue #5)
└── image_codec.rs   # ✅ PureRustCodec: image crate, Lanczos3, EXIF orientation kept (issue #7)
```

### Data model principles (hcg-format)

- **Only one mutation exists:** `HcgFile::set_image_path`. Raw model bytes are not public, so outer layers cannot change anything else (domain rule 1 enforced by types).
- Everything the tool never changes is **kept as raw bytes** and written back unchanged: fixed records, page headers, sequence tails, texts, RichText, cues, thumbnails, embedded payloads. Roundtrip is byte-identical by construction; nothing is re-overlaid from parsed fields.
- Image paths are stored as UTF-16 code units so even malformed UTF-16 roundtrips. The SafeBMP size field is derived from the payload on write.
- Only file version 0x78 is supported. Other versions are rejected with `UnsupportedVersion`; the report advises re-saving the file in ez-CGER, which writes 0x78.
- Contracts C1–C7 are listed in issue #3 and tested in `crates/hcg-format/tests/samples.rs`.

### Processing flow

1. **[adapter]** `read_zip`: zip -> flat file list. Entry names are UTF-8 when flagged or valid UTF-8, otherwise CP949. Limits are checked on bytes actually inflated.
   **[use case]** `pair_bundles`: each `*.hcg` with its same-named folder (case-insensitive, at any depth); clutter such as `__MACOSX/` is dropped, other leftovers are reported.
2. **[use case: analyze]** Parse the hcg, collect every path-type SafeBMP, and match it to a folder file by `ID + file name`. Record missing files. Run `optimize_image` per folder file (policy: fit 1920×1080, ≤ 1 MiB by default, PNG if alpha or a possible key colour, else JPEG from quality 90 down). Also collect non-CUT transitions and version warnings.
3. **[use case: apply]** Write each `Converted` result. If `output_file_name` reports an extension change, update the SafeBMP path (UTF-16) and length, recompute the ID, and build the new file name; keep the stamp. Write the hcg, re-parse it, and check that **all fields other than paths are equal**.
4. **[use case]** `output_files`: `<stem>_optimized/` with `<stem>.hcg`, `<stem>/`, `report.txt` per bundle. **[adapter]** `write_zip`: UTF-8 names, fixed timestamps (deterministic), JPEG/PNG stored, the rest deflated.
5. **[frontend]** The web page offers a download; the CLI writes a new file. The original is never overwritten.

## Verification strategy

| Target | Method |
|---|---|
| Layer rules | `check_layers.py` (preflight) |
| Reference implementation | `reference_roundtrip.sh`: Python roundtrip over all samples (preflight) |
| hcg-format | parse -> write byte-identical for every sample; truncated/mutated input never panics; edits change only size + payload; results equal `reference_summary.json` |
| Image ID | unit tests against sample folder file names |
| hcg-app | fast tests of policy, path updates, and report using a fake `ImageCodec` (test double) |
| Integration | zip `project/samples/layered.hcg` + `layered/` (and `speedtest`) and run the CLI end to end: output images ≤ 1920×1080 and ≤ 1 MB, non-path fields unchanged |

## Deployment

- **CI (`.github/workflows/ci.yml`):** runs `preflight.sh` on every push and PR.
- **Pages (`.github/workflows/pages.yml`):** on push to `main`, builds `dist/` with `build_web.sh` and deploys to GitHub Pages. Set Pages Source to "GitHub Actions" in the repository settings.
- **CLI:** for development and testing only; not distributed. Run with `cargo run -p hcg-cli -- <zip>`.
