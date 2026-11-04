//! hcg-format: domain layer. Model, parser/writer and image-folder ID for ez-CGER `.hcg` files.
//!
//! Rule: no dependency on the file system, network, threads, clocks, or image/zip libraries.
//! This crate only converts bytes to a model and back.
//!
//! Public surface:
//! - [`parse`] bytes -> [`HcgFile`] (version 0x78 only; never panics)
//! - [`HcgFile::to_bytes`] -> bytes (byte-identical for an unmodified file)
//! - [`HcgFile::pages`], [`HcgFile::images`] for reading
//! - [`HcgFile::set_image_path`]: the only mutation
//!
//! Format spec: `project/docs/HCG_FORMAT.md`. Reference implementation: `project/reference/*.py`.

mod edit;
mod error;
pub mod image_id;
mod model;
mod parse;
mod reader;
mod write;

pub use edit::{
    ImageRef, ImageRole, ImageSlot, ImageSource, MAX_PATH_UNITS, PageInfo, PageTransition,
    TransitionKind,
};
pub use error::{EditError, ParseError, ParseErrorKind};
pub use image_id::{image_id, image_id_utf16, subfolder_file_name};
pub use model::HcgFile;
pub use parse::{SUPPORTED_VERSION, parse};
