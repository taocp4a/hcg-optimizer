//! hcg-adapters: default implementations of what `hcg-app` needs from the outside world.
//!
//! - [`archive`]: zip <-> flat file list ([`hcg_app::package::InputFile`] /
//!   [`hcg_app::package::OutputFile`]), UTF-8 and CP949 entry names
//! - [`image_codec`]: [`hcg_app::ports::ImageCodec`] with the pure-Rust `image` crate
//! - [`pdf`]: page overview ([`hcg_app::overview::Overview`]) -> PDF
//!
//! Everything here must build for both native and wasm32 targets (no C bindings).

pub mod archive;
pub mod image_codec;
pub mod pdf;
