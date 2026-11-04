# Dependencies

Principle: use **only pure-Rust crates that build for WASM (wasm32-unknown-unknown)**. No C library bindings. Versions are managed in `[workspace.dependencies]` of the root `Cargo.toml`, and `Cargo.lock` is committed.

## Toolchain

| Item | Version | Notes |
|---|---|---|
| Rust | 1.97.0 | Pinned by `rust-toolchain.toml`, with rustfmt and clippy |
| Target | `wasm32-unknown-unknown` | Installed automatically from the same file |
| wasm-bindgen-cli | 0.2.100 | Must **exactly** match the `wasm-bindgen` crate. `cargo install wasm-bindgen-cli --version 0.2.100 --locked` |
| Python | 3.10+ | Only for the reference roundtrip check in preflight. Standard library only |

## Allowed per layer

- **Domain (`hcg-format`) and application (`hcg-app`):** pure computation crates only (crc32fast, thiserror, ...). `check_layers.py` blocks I/O and platform crates such as zip, image, wasm-bindgen.
- **Adapters (`hcg-adapters`):** zip, image, encoding_rs, etc. Must build for both native and wasm32.
- **Frontends (`hcg-cli`, `hcg-wasm`):** crates specific to that environment (wasm-bindgen, clap if needed, ...).

## In use

| Crate | Used by | Why |
|---|---|---|
| `crc32fast` 1.4 | hcg-format | Image ID (supports a custom initial value 'HDCG' via `new_with_initial`) |
| `thiserror` 2 | hcg-format, hcg-app, hcg-adapters | Error types |
| `serde` 1 (derive) | hcg-app | Serialize the structured report for viewers |
| `serde_json` 1 | hcg-app; hcg-format, hcg-adapters (dev only) | Report JSON for viewers; tests read JSON fixtures |
| `zip` 9.3 | hcg-adapters | Read/write zip. `default-features = false` + `deflate-flate2` only (no C libraries, no bzip2/zstd/lzma/AES) |
| `flate2` 1 | hcg-adapters | Selects the pure-Rust `rust_backend` (miniz_oxide) for zip's deflate, so it builds for wasm32 |
| `encoding_rs` 0.8 | hcg-adapters | Decode CP949 entry names (`EUC_KR` = WHATWG's windows-949) |
| `image` 0.25 | hcg-adapters | JPEG/PNG/BMP decode, Lanczos3 resize, JPEG/PNG encode, EXIF orientation. `default-features = false` + `jpeg`, `png`, `bmp` only (pure Rust: zune-jpeg, png) |
| `crc32fast` 1.4 | hcg-adapters (dev only) | Tests craft zips with chosen raw names and flags |
| `wasm-bindgen` =0.2.100 | hcg-wasm | JS bindings. Pinned with `=` to match the CLI version |

## Planned (add when needed and update this table)

| Crate | Used by | Why / notes |
|---|---|---|
| `js-sys` / `web-sys` | hcg-wasm | Only if needed, e.g. for progress callbacks |

## External material

- **The vendor's program, DLLs, and manual are not committed or redistributed.** The format spec (`HCG_FORMAT.md`) describes file structure and field meanings only.
- `project/samples/` contains test files made by the user in the app.
