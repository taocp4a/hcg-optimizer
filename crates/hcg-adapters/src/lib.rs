//! hcg-adapters: default implementations of the `hcg-app` ports.
//!
//! - zip <-> [`hcg_app::bundle::Bundle`] (UTF-8 / CP949 file names) — `zip`, `encoding_rs` planned
//! - pure-Rust [`hcg_app::ports::ImageCodec`] — `image` crate planned
//!
//! Everything here must build for both native and wasm32 targets (no C bindings).
