//! In-memory model. Everything the tool never changes stays in the source bytes and is written
//! back unchanged, so a parse/write roundtrip is byte-identical by construction (contract C1).
//! The model only records byte ranges into the source, so parsing copies nothing large (issue
//! #13): a 200-page file carries ~66 MB of thumbnails. The only mutable data is the path payload
//! of a SafeBMP (see `edit.rs`).

use std::borrow::Cow;
use std::ops::Range;

/// A parsed `.hcg` file (version 0x78). Build it with [`crate::parse`] (borrows the bytes) or
/// [`crate::parse_owned`] (takes ownership of them).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HcgFile<'a> {
    /// The source bytes; every `Range` in the model points into them.
    pub(crate) data: Cow<'a, [u8]>,
    /// §2 FileHeader (0x28 B), including the meaningless bytes 0x0C–0x27.
    pub(crate) header: [u8; 0x28],
    /// §3 DocHeader (0x20 B).
    pub(crate) doc_header: [u8; 0x20],
    /// §1 global Background layer.
    pub(crate) global_layer: Layer,
    pub(crate) pages: Vec<Page>,
}

/// §5 Page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Page {
    /// Page header (0x6C B): transition, resolution, cue count, thumbnail flag.
    pub(crate) header: [u8; 0x6C],
    pub(crate) layers: Vec<Layer>,
    /// Cue records followed by the thumbnail BMP, raw.
    pub(crate) trailer: Range<usize>,
    /// The thumbnail BMP inside `trailer`, when the page has one (§5 hdr+2).
    pub(crate) thumbnail: Option<Range<usize>>,
}

/// §4 Layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Layer {
    /// Layer header (0x18 B); +0 is the object count.
    pub(crate) header: [u8; 0x18],
    pub(crate) objects: Vec<Object>,
}

/// §6 Object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Object {
    /// Fixed record (0xF8 B) + trailing i32 array + sub header (0x20 B), raw.
    pub(crate) head: Range<usize>,
    /// Present when sub.hasImage != 0.
    pub(crate) image: Option<SafeBmp>,
    /// sub.nImages SafeBMPs (sequence or image list).
    pub(crate) images: Vec<SafeBmp>,
    /// Sequence tail, points, texts, external path + stamp, RichText, raw.
    pub(crate) rest: Range<usize>,
}

/// §6.2 SafeBMP block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SafeBmp {
    /// Header bytes 0x10–0x1F (flag, stamp, reserved), raw.
    pub(crate) flags_stamp: [u8; 0x10],
    /// §6.2 +0x14 original file modification time.
    pub(crate) stamp: u64,
    pub(crate) payload: Payload,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Payload {
    /// size <= 0: no payload. The original size value is preserved.
    Empty { size: i64 },
    /// flag != 0: UTF-16LE path, kept as code units so malformed UTF-16 still roundtrips.
    Path(Vec<u16>),
    /// flag == 0: raw image file bytes (read-only), as a range of the source.
    Embedded(Range<usize>),
}

impl HcgFile<'_> {
    /// File version from the header. Always [`crate::SUPPORTED_VERSION`] for a parsed file.
    pub fn version(&self) -> i32 {
        crate::reader::i32_at(&self.header, 0x08)
    }
}

impl<'a> HcgFile<'a> {
    /// Takes ownership of the source bytes, so the file no longer borrows anything.
    pub fn into_owned(self) -> HcgFile<'static> {
        HcgFile {
            data: Cow::Owned(self.data.into_owned()),
            header: self.header,
            doc_header: self.doc_header,
            global_layer: self.global_layer,
            pages: self.pages,
        }
    }

    /// The source bytes the file was parsed from (not including edits; see
    /// [`HcgFile::to_bytes`]). Free when the file owns them.
    pub fn into_source(self) -> Vec<u8> {
        self.data.into_owned()
    }
}
