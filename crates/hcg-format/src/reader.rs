//! Offset-tracking reader. Every read checks the remaining length first, so a length field taken
//! from the file can never cause an out-of-bounds access or an oversized allocation (contracts
//! C3/C4).

use crate::error::{ParseError, ParseErrorKind};

pub(crate) struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    context: Vec<String>,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            context: Vec::new(),
        }
    }

    pub(crate) fn pos(&self) -> usize {
        self.pos
    }

    pub(crate) fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    /// Builds an error at `offset` with the current context path.
    pub(crate) fn error(&self, offset: usize, kind: ParseErrorKind) -> ParseError {
        ParseError {
            offset,
            context: self.context.join(" / "),
            kind,
        }
    }

    /// Runs `f` with `label` pushed onto the context path.
    pub(crate) fn scope<T>(
        &mut self,
        label: impl Into<String>,
        f: impl FnOnce(&mut Self) -> Result<T, ParseError>,
    ) -> Result<T, ParseError> {
        self.context.push(label.into());
        let result = f(self);
        self.context.pop();
        result
    }

    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8], ParseError> {
        let available = self.remaining();
        if n > available {
            return Err(self.error(
                self.pos,
                ParseErrorKind::UnexpectedEof {
                    needed: n,
                    available,
                },
            ));
        }
        let bytes = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(bytes)
    }

    pub(crate) fn array<const N: usize>(&mut self) -> Result<[u8; N], ParseError> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }

    pub(crate) fn i32(&mut self) -> Result<i32, ParseError> {
        Ok(i32::from_le_bytes(self.array()?))
    }

    /// The next `n` bytes without consuming them, if available.
    pub(crate) fn peek(&self, n: usize) -> Option<&'a [u8]> {
        self.data.get(self.pos..self.pos.checked_add(n)?)
    }

    /// Validates a count/length field read from the file at `at`: it must not be negative.
    pub(crate) fn count(
        &self,
        value: i64,
        field: &'static str,
        at: usize,
    ) -> Result<usize, ParseError> {
        usize::try_from(value)
            .map_err(|_| self.error(at, ParseErrorKind::InvalidLength { field, value }))
    }

    /// `count * elem_size` as a byte length, rejecting overflow.
    pub(crate) fn byte_len(
        &self,
        count: usize,
        elem_size: usize,
        field: &'static str,
        at: usize,
    ) -> Result<usize, ParseError> {
        count.checked_mul(elem_size).ok_or_else(|| {
            let value = i64::try_from(count).unwrap_or(i64::MAX);
            self.error(at, ParseErrorKind::InvalidLength { field, value })
        })
    }
}

/// Little-endian field helpers for fixed raw records. Offsets are compile-time constants from
/// HCG_FORMAT.md, so out-of-range indexing would be a programming error, not a data error.
pub(crate) fn i32_at(buf: &[u8], off: usize) -> i32 {
    let mut b = [0u8; 4];
    b.copy_from_slice(&buf[off..off + 4]);
    i32::from_le_bytes(b)
}

pub(crate) fn u32_at(buf: &[u8], off: usize) -> u32 {
    let mut b = [0u8; 4];
    b.copy_from_slice(&buf[off..off + 4]);
    u32::from_le_bytes(b)
}

pub(crate) fn i64_at(buf: &[u8], off: usize) -> i64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&buf[off..off + 8]);
    i64::from_le_bytes(b)
}

pub(crate) fn u64_at(buf: &[u8], off: usize) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&buf[off..off + 8]);
    u64::from_le_bytes(b)
}
