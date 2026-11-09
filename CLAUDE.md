# CLAUDE.md: agent onboarding

> **This file and `AGENTS.md` must be identical.** If you edit one, run `cp CLAUDE.md AGENTS.md`.
> `project/scripts/preflight.sh` checks that they match.

## 1. Product goal

A tool that **shrinks large images inside `.hcg` project files** of ez-CGER (LUMANTEK), a church subtitle/presentation program, **to remove page-transition lag**.

- **Users:** people who run Windows PCs at churches. It must work without installing anything.
- **Frontends:** one Rust core, several thin frontends.
  1. **WASM + static GitHub Pages site (for users):** everything runs in the browser, no server. **Files never leave the machine.**
  2. **CLI (development and testing):** runs the same use cases from a terminal.
  3. A GUI or other frontends are **not planned**, but the clean architecture keeps it possible: adding one means adding one frontend crate.
- **Input:** one zip containing `xxx.hcg` and an image folder with the same name, `xxx/`. Entry names may be UTF-8 or CP949.
- **Output:** one zip containing the fixed hcg, the shrunk image folder, and an inspection report. **Never overwrite the original.**

Full requirements: [`project/docs/REQUIREMENTS.md`](project/docs/REQUIREMENTS.md). Read it before starting work.

## 2. Domain rules (must follow)

1. **Preserve bytes:** in the hcg, change only image paths and their length fields. Keep **every other byte as is** (including the garbage in FileHeader 0x0C–0x27).
2. **Roundtrip:** for every sample, `parse → write` must produce output byte-identical to the input.
3. **Never skip silently:** unparsed bytes, unknown versions, or trailing data are **errors**.
4. **Image ID:** the 8 hex digits at the start of each image-folder file name are `CRC32(UTF-16LE(IntToHex(stamp,4) + lower(path)), seed 'HDCG')`. **If a path or extension changes (png→jpg), recompute the ID and rename the folder file.** Implemented in `crates/hcg-format/src/image_id.rs`.
5. **Resize:** keep the aspect ratio and fit within 1920×1080; leave smaller images alone. Target ≤ 1 MB. Images without transparency become JPEG (lower quality until ≤ 1 MB); PNGs that actually use transparency stay PNG.
6. **Never create embedded images (SafeBMP flag 0).** The app drops them when it re-saves the file. Reading them is supported.
7. Do not commit the vendor's program, DLLs, or manual. Docs and comments describe file structure and field meanings only.

Format spec: [`project/docs/HCG_FORMAT.md`](project/docs/HCG_FORMAT.md). Check the evidence tags ([confirmed], [unverified], [guess]). The verified reference implementation is the Python code in [`project/reference/`](project/reference/). **It is the oracle for the Rust port.**

## 3. Project tree

```
.
├── CLAUDE.md / AGENTS.md      # this document (the two must be identical)
├── README.md                  # human-facing introduction
├── LICENSE-MIT / LICENSE-APACHE  # dual license (MIT OR Apache-2.0)
├── Cargo.toml                 # Rust workspace (dependency versions live here)
├── rust-toolchain.toml        # pins Rust 1.97.0 + wasm32-unknown-unknown
├── crates/                    # clean-architecture layers (dependencies point inward only)
│   ├── hcg-format/            # [domain] hcg model, parser/writer, image ID. Pure, no I/O
│   ├── hcg-app/               # [application] analyze/apply, resize policy, report, port traits
│   ├── hcg-adapters/          # [adapters] zip <-> Bundle, pure-Rust ImageCodec, overview PDF
│   ├── hcg-cli/               # [frontend] `hcg-optimizer` CLI (development and testing)
│   └── hcg-wasm/              # [frontend] wasm-bindgen bindings (web)
├── web/                       # static GitHub Pages front end: index.html, main.js (display), worker.js (WASM session), style.css
├── project/                   # everything that is not code
│   ├── docs/                  # requirements, format spec, report format, architecture, dependencies
│   ├── scripts/               # preflight.sh, check_layers.py, build_web.sh, web_e2e.sh(+.js), reference_roundtrip.sh, reference_summary.py,
│   │                          # bench_memory.sh + make_bench_sample.py (time/memory of optimize and overview PDF on a 204-page project)
│   ├── bench/                 # benchmark crate used by bench_memory.sh (not a workspace member)
│   ├── reference/             # Python reference implementation (hcg_parse.py, hcg_write.py)
│   └── samples/               # all 91 test .hcg files, flat, with same-named image folders, + a CP949 zip
│       ├── README.md          # what each sample is for (read before adding one)
│       ├── layered/, speedtest/  # large images (12 MB JPGs, 6000×4000 photo, 6 MB PNG); example tool inputs
│       └── reference_summary.json  # Python reference output; the Rust tests compare against it
└── .github/workflows/         # ci.yml (preflight), pages.yml (GitHub Pages deploy)
```

Architecture: [`project/docs/ARCHITECTURE.md`](project/docs/ARCHITECTURE.md). Dependencies: [`project/docs/DEPENDENCIES.md`](project/docs/DEPENDENCIES.md).

## 4. Development workflow

```bash
cargo test --workspace                 # unit/integration tests
project/scripts/preflight.sh           # ★ required before every PR. No PR if it fails
project/scripts/build_web.sh           # builds the static site into dist/ (needs wasm-bindgen-cli)
project/scripts/web_e2e.sh             # browser E2E (Playwright + Chromium): plan, overview PDF, optimize, download, no external requests
python3 -m http.server -d dist 8000    # preview the site locally
cargo run -p hcg-cli -- input.zip      # run the CLI (writes input_optimized.zip; report on stdout)
cargo run -p hcg-cli -- project/samples/speedtest.hcg -o /tmp/out.zip   # .hcg input + its folder
cargo run -p hcg-cli -- input.zip --dry-run   # plan only; --json for the viewer data; --help
cargo run -p hcg-cli -- input.zip --pdf       # page overview PDF from the thumbnails (--per-sheet 6|12|20)
```

**preflight.sh checks:** CLAUDE.md and AGENTS.md are identical, layer dependency rules (`check_layers.py`), `cargo fmt --check`, `clippy -D warnings`, `cargo test`, wasm32 build, Python reference roundtrip over the samples, and that `reference_summary.json` is up to date (regenerate with `project/scripts/reference_summary.py` after changing samples or the reference).

## 5. Coding style

- **Rust 2024 edition, rustfmt defaults (max_width 100).** No `unsafe` (enforced by a workspace lint). Clippy warnings are errors.
- **Respect the clean-architecture layers** (details in ARCHITECTURE.md):
  - `hcg-format` (domain) ← `hcg-app` (application) ← `hcg-adapters` (adapters) ← `hcg-cli`/`hcg-wasm` (frontends). **Inner layers know nothing about outer ones.**
  - Domain and application code do not use `std::fs`, the network, threads, clocks, or zip/image/wasm-bindgen. If outside functionality is needed, define a trait in `hcg-app::ports` and implement it in an adapter.
  - Frontend crates do I/O and presentation only. No processing logic there.
  - When adding a crate, register its layer in `LAYERS` in `project/scripts/check_layers.py`. Preflight fails otherwise.
- **Errors:** libraries return `Result<T, Error>`. Errors carry **the file offset and context** (`"SafeBMP magic mismatch at 0x1234"`). `unwrap()`/`expect()` only in tests. Never end processing with a panic.
- **Format code:** annotate offset constants with the HCG_FORMAT.md section and offset (e.g. `// §6.2 SafeBMP +0x10 flag`). Fields of unknown meaning are not interpreted; keep them as raw bytes.
- **Language:** everything in the repository is written in English (code, comments, docs, scripts, commit messages, CLI messages, the English report), **except** what users of the web page read: the web page (`web/`), the Korean report text (`crates/hcg-app/src/report/ko.rs`), and the session messages shown on the page. Those are Korean, formal and concise (합니다체, instructions in 하십시오체, no emoji), with the conclusion and the action first (issue #21).
- **Tests:** new format-handling code gets roundtrip tests based on `project/samples`. Computed values such as image IDs are tested against real sample folder file names.
- **Dependencies:** before adding one, check it builds for WASM (pure Rust) and record why in `project/docs/DEPENDENCIES.md`. Versions are managed in the workspace `Cargo.toml`.
- **Commits:** keep them small; messages say what changed and why.

## 6. Roadmap and status

The roadmap lives in the umbrella issue [#1](https://github.com/taocp4a/hcg-optimizer/issues/1), not in this file.

- Check #1 before starting work to see what is done and what comes next.
- Larger tasks get their own issue, attached to #1 as a sub-issue.
- When a task is finished, tick it in #1. Do not copy the task list into this file.
