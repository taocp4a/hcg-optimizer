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
│  hcg-adapters: zip <-> Bundle (UTF-8/CP949), pure-Rust ImageCodec     │
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
| `hcg-adapters` | adapters | zip <-> Bundle, ImageCodec implementation | hcg-app, hcg-format, zip/image/encoding_rs |
| `hcg-cli` | frontend | arguments, file I/O, terminal progress | all of the above |
| `hcg-wasm` | frontend | JS <-> Rust bytes, `JsError` conversion, on-page progress | all of the above + wasm-bindgen |

**`project/scripts/check_layers.py` enforces these rules** (preflight step 2):
- Every crate must be assigned a layer.
- No dependency on the same layer or an outer layer.
- Domain and application must not depend on I/O or platform crates (zip, image, wasm-bindgen, ...).

## Why the boundaries are drawn here

- **analyze and apply are separate.** First build a plan and report (changing nothing), then apply it. The web "this is what will be shrunk" confirmation, a CLI `--dry-run`, and a possible future GUI preview all share this code.
- **ImageCodec is a port (trait).** The default implementation uses the pure-Rust `image` crate. Heavy work such as a 6000×4000 photo may be faster with the browser's built-in encoder, so the web frontend can swap in its own implementation.
- **Progress is a port.** Each frontend shows progress and cancellation its own way while the use-case code stays single.
- **zip stays outside the use cases.** Use cases only know the in-memory `Bundle` (hcg bytes + list of image files). A different input form (e.g. picking a folder directly) only needs a new adapter.
- **hcg-format has no traits.** There is only one format, so it is concrete code. No needless abstraction.

## Module plan

```
hcg-format/src/
├── image_id.rs      # ✅ §6.3 CRC32 ID
├── reader.rs        # offset-tracking reader, coverage check (unhandled bytes -> Error)
├── model.rs         # HcgFile / Page / Layer / Object / SafeBmp / RichText
├── parse.rs         # port of reference/hcg_parse.py
└── write.rs         # port of reference/hcg_write.py

hcg-app/src/
├── bundle.rs        # ✅ Bundle / BundleFile
├── ports.rs         # ✅ ImageCodec, Progress, NoProgress
├── policy.rs        # 1920×1080, 1 MB, JPEG quality search order, keep PNG when alpha is used
├── analyze.rs       # Bundle -> Plan (image matching, missing files, transition and version warnings)
├── apply.rs         # execute the Plan: convert images, update paths/IDs, rewrite hcg, re-parse check
└── report.rs        # Plan/result -> report.txt (or html)

hcg-adapters/src/
├── archive.rs       # zip -> Vec<Bundle>, Bundle -> zip (CP949/UTF-8 names)
└── image_codec.rs   # ImageCodec implementation (image crate)
```

### Data model principles (hcg-format)

- Only fields with known meaning become struct fields. Fixed records (0xF8 object record, 0x6C page header, ...) are **kept as raw byte arrays**; the writer overlays known fields onto the raw bytes (the `rec_hex` approach of the Python reference).
- Length fields (sub.lenPath, SafeBMP size, ...) are recomputed from the actual data on write.
- Thumbnail BMPs, cue records, and embedded image payloads are preserved byte for byte.

### Processing flow

1. **[adapter]** zip -> `Vec<Bundle>`: pair each `*.hcg` with its same-named folder. Entry names are read as UTF-8 when the UTF-8 flag is set, otherwise as CP949.
2. **[use case: analyze]** Parse the hcg, collect every path-type SafeBMP, and match it to a folder file by `ID + file name`. Record missing files. Use `ImageCodec::inspect` for size and alpha, and build a per-image plan. Also collect non-CUT transitions and version warnings.
3. **[use case: apply]** Run `ImageCodec::resize_encode` per the plan. If the extension changes, update the SafeBMP path (UTF-16) and length, recompute the ID, and build the new file name; keep the stamp. Write the hcg, re-parse it, and check that **all fields other than paths are equal**.
4. **[adapter]** `Bundle` + report -> output zip.
5. **[frontend]** The web page offers a download; the CLI writes a new file. The original is never overwritten.

## Verification strategy

| Target | Method |
|---|---|
| Layer rules | `check_layers.py` (preflight) |
| Reference implementation | `reference_roundtrip.sh`: Python roundtrip over all samples (preflight) |
| hcg-format | parse -> write is byte-identical for every `project/samples/**/*.hcg`, with 100% coverage |
| Image ID | unit tests against sample folder file names |
| hcg-app | fast tests of policy, path updates, and report using a fake `ImageCodec` (test double) |
| Integration | zip `project/samples/large/` and run the CLI end to end: output images ≤ 1920×1080 and ≤ 1 MB, non-path fields unchanged |

## Deployment

- **CI (`.github/workflows/ci.yml`):** runs `preflight.sh` on every push and PR.
- **Pages (`.github/workflows/pages.yml`):** on push to `main`, builds `dist/` with `build_web.sh` and deploys to GitHub Pages. Set Pages Source to "GitHub Actions" in the repository settings.
- **CLI:** for development and testing only; not distributed. Run with `cargo run -p hcg-cli -- <zip>`.
