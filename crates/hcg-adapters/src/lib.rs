//! hcg-adapters: default implementations of what `hcg-app` needs from the outside world.
//!
//! - [`archive`]: zip <-> flat file list ([`hcg_app::package::InputFile`] /
//!   [`hcg_app::package::OutputFile`]), UTF-8 and CP949 entry names
//! - pure-Rust [`hcg_app::ports::ImageCodec`] — `image` crate planned
//!
//! Everything here must build for both native and wasm32 targets (no C bindings).

pub mod archive;
