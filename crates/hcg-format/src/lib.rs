//! hcg-format: domain layer. Model, parser/writer and image-folder ID for ez-CGER `.hcg` files.
//!
//! Rule: no dependency on the file system, network, threads, clocks, or image/zip libraries.
//! This crate only converts bytes to structs and structs back to bytes.
//!
//! Format spec: `project/docs/HCG_FORMAT.md`. Reference implementation: `project/reference/*.py`.

pub mod image_id;

pub use image_id::{image_id, subfolder_file_name};
