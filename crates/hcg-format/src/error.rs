//! Error types. Errors are data: the application layer turns them into report entries, so they
//! carry the byte offset and a human-readable context path.

use std::fmt;

/// Why parsing failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseErrorKind {
    #[error("bad magic (expected {expected:?})")]
    BadMagic { expected: &'static str },
    #[error("unsupported file version {0:#x} (only 0x78 is supported)")]
    UnsupportedVersion(i32),
    #[error("unexpected end of file (needed {needed} bytes, {available} available)")]
    UnexpectedEof { needed: usize, available: usize },
    #[error("invalid {field}: {value}")]
    InvalidLength { field: &'static str, value: i64 },
    #[error("{0} trailing bytes after the last page")]
    TrailingBytes(usize),
    #[error("invalid page thumbnail")]
    BadThumbnail,
}

/// A parse failure at a specific byte offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// Byte offset in the input where the problem was detected.
    pub offset: usize,
    /// Where in the structure it happened, e.g. `page 3 / layer 1 / object 5 / image`.
    pub context: String,
    pub kind: ParseErrorKind,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.context.is_empty() {
            write!(f, "{} at {:#x}", self.kind, self.offset)
        } else {
            write!(f, "{}: {} at {:#x}", self.context, self.kind, self.offset)
        }
    }
}

impl std::error::Error for ParseError {}

/// Why an image path edit was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    #[error("image slot does not exist in this file")]
    InvalidSlot,
    #[error("image is embedded or empty, not a path")]
    NotAPath,
    #[error("new path is empty")]
    EmptyPath,
    #[error("new path is too long ({0} UTF-16 units)")]
    PathTooLong(usize),
}
