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
| `wasm-bindgen` =0.2.100 | hcg-wasm | JS bindings. Pinned with `=` to match the CLI version |

## Planned (add when needed and update this table)

| Crate | Used by | Why / notes |
|---|---|---|
| `zip` | hcg-adapters | Read/write zip. `default-features = false` with only `deflate`, to stay WASM-compatible |
| `encoding_rs` | hcg-adapters | Decode CP949 (superset of EUC-KR) entry names |
| `image` | hcg-adapters | PNG/JPEG decode, resize, encode. `default-features = false`, only the `png` and `jpeg` features |
| `thiserror` | hcg-format, hcg-app | Error types |
| `js-sys` / `web-sys` | hcg-wasm | Only if needed, e.g. for progress callbacks |

## External material

- **The vendor's program, DLLs, and manual are not committed or redistributed.** The format spec (`HCG_FORMAT.md`) describes file structure and field meanings only.
- `project/samples/` contains test files made by the user in the app.
