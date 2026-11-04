//! hcg-app: application (use-case) layer.
//!
//! - `analyze(bundle) -> Plan`: decides what to shrink and how, and collects missing files and
//!   warnings. Changes nothing.
//! - `apply(bundle, plan) -> Bundle`: converts images according to the plan and updates hcg
//!   paths/IDs.
//!
//! Input and output are an in-memory [`bundle::Bundle`]. Adapters turn a zip into a flat file
//! list ([`package::InputFile`]); [`package::pair_bundles`] groups it into bundles and
//! [`package::output_files`] lays out the result.
//! Anything whose implementation may differ per runtime (image decode/encode, progress) is taken
//! through the traits in [`ports`].
//!
//! This crate depends on no internal crate other than `hcg-format` (checked by preflight).

pub mod bundle;
pub mod package;
pub mod policy;
pub mod ports;
