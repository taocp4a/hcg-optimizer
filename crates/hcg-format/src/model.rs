//! In-memory model. Everything the tool never changes is kept as raw bytes and written back
//! unchanged, so a parse/write roundtrip is byte-identical by construction (contract C1).
//! The only mutable data is the path payload of a SafeBMP (see `edit.rs`).

/// A parsed `.hcg` file (version 0x78). Build it with [`crate::parse`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HcgFile {
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
    pub(crate) trailer: Vec<u8>,
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
    pub(crate) head: Vec<u8>,
    /// Present when sub.hasImage != 0.
    pub(crate) image: Option<SafeBmp>,
    /// sub.nImages SafeBMPs (sequence or image list).
    pub(crate) images: Vec<SafeBmp>,
    /// Sequence tail, points, texts, external path + stamp, RichText, raw.
    pub(crate) rest: Vec<u8>,
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
    /// flag == 0: raw image file bytes (read-only).
    Embedded(Vec<u8>),
}

impl HcgFile {
    /// File version from the header. Always [`crate::SUPPORTED_VERSION`] for a parsed file.
    pub fn version(&self) -> i32 {
        crate::reader::i32_at(&self.header, 0x08)
    }
}
