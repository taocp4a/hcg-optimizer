//! zip <-> flat file list ([`InputFile`] / [`OutputFile`]). Everything stays in memory; nothing
//! is ever extracted to disk, so zip-slip cannot happen.

use std::collections::HashSet;
use std::io::{Cursor, Read, Write};

use hcg_app::package::{InputFile, NameEncoding, OutputFile};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

/// General purpose bit 11: entry name and comment are UTF-8.
const UTF8_FLAG: u16 = 1 << 11;

/// Resource limits for reading untrusted zips. Checked against bytes actually inflated, not the
/// sizes the archive declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZipLimits {
    pub max_entries: usize,
    pub max_total_bytes: u64,
}

impl Default for ZipLimits {
    /// 10,000 entries and 1 GiB in total: far above real projects, safely below wasm32 memory.
    fn default() -> Self {
        Self {
            max_entries: 10_000,
            max_total_bytes: 1 << 30,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ArchiveError {
    #[error("not a valid zip file: {0}")]
    Invalid(String),
    #[error("{path}: encrypted zip entries are not supported")]
    Encrypted { path: String },
    #[error("too many entries in the zip (limit {limit})")]
    TooManyEntries { limit: usize },
    #[error("zip contents are too large (limit {limit} bytes)")]
    TooLarge { limit: u64 },
    #[error("{path}: unsafe path in zip")]
    UnsafePath { path: String },
    #[error("{path}: the zip contains this path twice")]
    DuplicatePath { path: String },
    #[error("the zip lists {declared} entries but only {distinct} distinct names")]
    DuplicateEntries { declared: u64, distinct: usize },
    #[error("entry name is neither UTF-8 nor CP949: {raw_hex}")]
    UndecodableName { raw_hex: String },
}

impl From<zip::result::ZipError> for ArchiveError {
    fn from(e: zip::result::ZipError) -> Self {
        ArchiveError::Invalid(e.to_string())
    }
}

/// Reads every file entry of a zip. Directory entries are skipped. Never panics on malformed
/// input.
pub fn read_zip(bytes: &[u8], limits: &ZipLimits) -> Result<Vec<InputFile>, ArchiveError> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))?;
    if archive.len() > limits.max_entries {
        return Err(ArchiveError::TooManyEntries {
            limit: limits.max_entries,
        });
    }
    // The zip crate keeps only one entry per exact name; detect the dropped ones.
    if let Some(declared) = declared_entry_count(bytes)
        && declared != archive.len() as u64
    {
        return Err(ArchiveError::DuplicateEntries {
            declared,
            distinct: archive.len(),
        });
    }
    let mut files: Vec<InputFile> = Vec::new();
    let mut seen = HashSet::new();
    let mut total: u64 = 0;
    for i in 0..archive.len() {
        // Metadata first, from the raw entry: decrypting or inflating is not needed for it.
        let (path, name_encoding) = {
            let raw = archive.by_index_raw(i)?;
            let utf8_flag = raw.flags().as_u16() & UTF8_FLAG != 0;
            let (name, name_encoding) = decode_name(raw.name_raw(), utf8_flag)?;
            if raw.is_dir() || name.ends_with('/') || name.ends_with('\\') {
                continue;
            }
            let path = normalize_path(&name)?;
            if raw.encrypted() {
                return Err(ArchiveError::Encrypted { path });
            }
            (path, name_encoding)
        };
        if !seen.insert(path.clone()) {
            return Err(ArchiveError::DuplicatePath { path });
        }
        let mut entry = archive.by_index(i)?;

        // Read at most one byte past the remaining budget, whatever the entry claims.
        let budget = limits.max_total_bytes - total;
        let mut data = Vec::new();
        (&mut entry)
            .take(budget.saturating_add(1))
            .read_to_end(&mut data)
            .map_err(|e| ArchiveError::Invalid(format!("{path}: {e}")))?;
        total += data.len() as u64;
        if total > limits.max_total_bytes {
            return Err(ArchiveError::TooLarge {
                limit: limits.max_total_bytes,
            });
        }
        files.push(InputFile {
            path,
            bytes: data,
            name_encoding,
        });
    }
    Ok(files)
}

/// Writes files into a new zip. Names are UTF-8 (flagged when non-ASCII). Timestamps are fixed
/// (1980-01-01), so the same input always gives the same bytes. JPEG/PNG are stored as is
/// (already compressed); everything else is deflated.
pub fn write_zip(files: &[OutputFile]) -> Result<Vec<u8>, ArchiveError> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for file in files {
        let method = if is_precompressed(&file.path) {
            CompressionMethod::Stored
        } else {
            CompressionMethod::Deflated
        };
        let options = SimpleFileOptions::default()
            .compression_method(method)
            .last_modified_time(DateTime::default())
            .unix_permissions(0o644)
            .large_file(file.bytes.len() as u64 >= u64::from(u32::MAX));
        writer.start_file(file.path.as_str(), options)?;
        writer
            .write_all(&file.bytes)
            .map_err(|e| ArchiveError::Invalid(format!("{}: {e}", file.path)))?;
    }
    Ok(writer.finish()?.into_inner())
}

/// Total entry count from the end-of-central-directory record, if present and not zip64.
fn declared_entry_count(bytes: &[u8]) -> Option<u64> {
    const EOCD: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
    const EOCD_LEN: usize = 22;
    // The record sits at the end, followed by a comment of at most 64 KiB.
    let last = bytes.len().checked_sub(EOCD_LEN)?;
    let first = last.saturating_sub(usize::from(u16::MAX));
    let at = (first..=last).rev().find(|&i| bytes[i..i + 4] == EOCD)?;
    let total = u16::from_le_bytes([bytes[at + 10], bytes[at + 11]]);
    (total != u16::MAX).then_some(u64::from(total))
}

fn decode_name(raw: &[u8], utf8_flag: bool) -> Result<(String, NameEncoding), ArchiveError> {
    let undecodable = || ArchiveError::UndecodableName {
        raw_hex: raw.iter().map(|b| format!("{b:02x}")).collect(),
    };
    if raw.is_ascii() {
        return Ok((
            String::from_utf8_lossy(raw).into_owned(),
            NameEncoding::Ascii,
        ));
    }
    if let Ok(s) = std::str::from_utf8(raw) {
        let encoding = if utf8_flag {
            NameEncoding::Utf8Flag
        } else {
            NameEncoding::Utf8Guessed
        };
        return Ok((s.to_string(), encoding));
    }
    if utf8_flag {
        return Err(undecodable());
    }
    let decoded = encoding_rs::EUC_KR
        .decode_without_bom_handling_and_without_replacement(raw)
        .ok_or_else(undecodable)?;
    Ok((decoded.into_owned(), NameEncoding::Cp949))
}

/// `\` -> `/`, drops leading `/`, empty and `.` components; rejects `..` and drive letters.
fn normalize_path(name: &str) -> Result<String, ArchiveError> {
    let unified = name.replace('\\', "/");
    let mut parts = Vec::new();
    for part in unified.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                return Err(ArchiveError::UnsafePath {
                    path: name.to_string(),
                });
            }
            p if p.contains(':') => {
                return Err(ArchiveError::UnsafePath {
                    path: name.to_string(),
                });
            }
            p => parts.push(p),
        }
    }
    if parts.is_empty() {
        return Err(ArchiveError::UnsafePath {
            path: name.to_string(),
        });
    }
    Ok(parts.join("/"))
}

fn is_precompressed(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(_, ext)| {
        ["jpg", "jpeg", "png"]
            .iter()
            .any(|e| ext.eq_ignore_ascii_case(e))
    })
}
