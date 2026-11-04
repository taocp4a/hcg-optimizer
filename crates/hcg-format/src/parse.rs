//! Parser: reads the stream in HCG_FORMAT.md §1–§6 order. Only version 0x78 is supported, so no
//! version branches exist here. Every byte is either kept raw or interpreted; anything left over
//! is an error.

use crate::error::{ParseError, ParseErrorKind};
use crate::model::{HcgFile, Layer, Object, Page, Payload, SafeBmp};
use crate::reader::{Reader, i32_at, i64_at, u32_at, u64_at};

/// The only file version this crate reads and writes (§2 FileHeader +0x08).
pub const SUPPORTED_VERSION: i32 = 0x78;

const FILE_MAGIC: &[u8; 8] = b"HDCGData";
const SAFEBMP_MAGIC: &[u8; 8] = b"SafeBMP ";
/// §6.1 fixed record size.
const RECORD_SIZE: usize = 0xF8;
/// §6.1 +0xF0 length of the trailing i32 array.
const RECORD_ARRAY_LEN: usize = 0xF0;
/// Size of one RichText character record before its font-name WStr (§6.4).
const RICH_CHAR_RECORD: usize = 28;
/// Smallest valid BMP (file header only).
const BMP_MIN_SIZE: u32 = 14;

/// Parses a complete `.hcg` file. Never panics; any unexpected input is a [`ParseError`].
pub fn parse(bytes: &[u8]) -> Result<HcgFile, ParseError> {
    let mut r = Reader::new(bytes);

    let header = r.scope("file header", |r| {
        let at = r.pos();
        let header: [u8; 0x28] = r.array()?;
        if &header[..8] != FILE_MAGIC {
            return Err(r.error(
                at,
                ParseErrorKind::BadMagic {
                    expected: "HDCGData",
                },
            ));
        }
        let version = i32_at(&header, 0x08);
        if version != SUPPORTED_VERSION {
            return Err(r.error(at + 0x08, ParseErrorKind::UnsupportedVersion(version)));
        }
        Ok(header)
    })?;
    let doc_header = r.scope("doc header", |r| r.array())?;
    let global_layer = r.scope("global layer", read_layer)?;

    let at = r.pos();
    let raw = r.i32()?;
    let page_count = r.count(raw.into(), "page count", at)?;
    let mut pages = Vec::new();
    for i in 0..page_count {
        pages.push(r.scope(format!("page {}", i + 1), read_page)?);
    }

    if r.remaining() > 0 {
        return Err(r.error(r.pos(), ParseErrorKind::TrailingBytes(r.remaining())));
    }
    Ok(HcgFile {
        header,
        doc_header,
        global_layer,
        pages,
    })
}

/// §5 Page.
fn read_page(r: &mut Reader<'_>) -> Result<Page, ParseError> {
    let header: [u8; 0x6C] = r.array()?;

    let at = r.pos();
    let raw = r.i32()?;
    let layer_count = r.count(raw.into(), "layer count", at)?;
    let mut layers = Vec::new();
    for i in 0..layer_count {
        layers.push(r.scope(format!("layer {}", i + 1), read_layer)?);
    }

    let trailer_start = r.pos();
    // hdr+8: cue records, 40 B each; the app skips them.
    let cue_count = r.count(i32_at(&header, 0x08).into(), "cue count", trailer_start)?;
    let cue_len = r.byte_len(cue_count, 40, "cue count", trailer_start)?;
    r.scope("cues", |r| r.take(cue_len).map(|_| ()))?;
    // hdr+2: thumbnail BMP present.
    if header[2] != 0 {
        r.scope("thumbnail", read_thumbnail)?;
    }
    let trailer = r.since(trailer_start).to_vec();

    Ok(Page {
        header,
        layers,
        trailer,
    })
}

fn read_thumbnail(r: &mut Reader<'_>) -> Result<(), ParseError> {
    let at = r.pos();
    let head = r.peek(6).ok_or_else(|| {
        r.error(
            at,
            ParseErrorKind::UnexpectedEof {
                needed: 6,
                available: r.remaining(),
            },
        )
    })?;
    if &head[..2] != b"BM" {
        return Err(r.error(at, ParseErrorKind::BadThumbnail));
    }
    let size = u32_at(head, 2);
    if size < BMP_MIN_SIZE {
        return Err(r.error(at, ParseErrorKind::BadThumbnail));
    }
    let len = r.count(size.into(), "thumbnail size", at)?;
    r.take(len)?;
    Ok(())
}

/// §4 Layer.
fn read_layer(r: &mut Reader<'_>) -> Result<Layer, ParseError> {
    let at = r.pos();
    let header: [u8; 0x18] = r.array()?;
    let object_count = r.count(i32_at(&header, 0).into(), "object count", at)?;
    let mut objects = Vec::new();
    for i in 0..object_count {
        objects.push(r.scope(format!("object {}", i + 1), read_object)?);
    }
    Ok(Layer { header, objects })
}

/// §6 Object.
fn read_object(r: &mut Reader<'_>) -> Result<Object, ParseError> {
    let start = r.pos();
    let record = r.take(RECORD_SIZE)?;
    let array_at = start + RECORD_ARRAY_LEN;
    let array_len = r.count(
        i32_at(record, RECORD_ARRAY_LEN).into(),
        "record array length",
        array_at,
    )?;
    let array_bytes = r.byte_len(array_len, 4, "record array length", array_at)?;
    r.take(array_bytes)?;

    // sub header (0x20): +0 hasImage, +1 hasRich, +4 nPoints, +8 lenText, +0xC lenText2,
    // +0x10 lenPath, +0x14 nImages, +0x18 len4 (unused for version 0x78).
    let sub_at = r.pos();
    let sub: [u8; 0x20] = r.array()?;
    let has_image = sub[0] != 0;
    let has_rich = sub[1] != 0;
    let field =
        |off: usize, name: &'static str| r.count(i32_at(&sub, off).into(), name, sub_at + off);
    let points = field(0x04, "point count")?;
    let text_len = field(0x08, "text length")?;
    let text2_len = field(0x0C, "text2 length")?;
    let path_len = field(0x10, "path length")?;
    let image_count = field(0x14, "image count")?;
    let head = r.since(start).to_vec();

    let image = if has_image {
        Some(r.scope("image", read_safebmp)?)
    } else {
        None
    };
    let mut images = Vec::new();
    for i in 0..image_count {
        images.push(r.scope(format!("sequence image {}", i + 1), read_safebmp)?);
    }

    let rest_start = r.pos();
    r.scope("sequence tail", |r| r.take(16).map(|_| ()))?; // §6.1b i32×4
    let at = r.pos();
    let len = r.byte_len(points, 12, "point count", at)?; // §6.1c
    r.scope("points", |r| r.take(len).map(|_| ()))?;
    let len = r.byte_len(text_len, 2, "text length", r.pos())?;
    r.scope("text", |r| r.take(len).map(|_| ()))?;
    let len = r.byte_len(text2_len, 2, "text2 length", r.pos())?;
    r.scope("text2", |r| r.take(len).map(|_| ()))?;
    if path_len > 0 {
        let len = r.byte_len(path_len, 2, "path length", r.pos())?;
        r.scope("external path", |r| {
            r.take(len).and_then(|_| r.take(8)).map(|_| ())
        })?;
    }
    if has_rich {
        r.scope("rich text", skip_rich_text)?;
    }
    let rest = r.since(rest_start).to_vec();

    Ok(Object {
        head,
        image,
        images,
        rest,
    })
}

/// §6.4 RichText: WStr text, then one record per UTF-16 unit of the text. Walked to find its
/// length; the content is kept raw and not interpreted.
fn skip_rich_text(r: &mut Reader<'_>) -> Result<(), ParseError> {
    let chars = read_wstr_len(r, "rich text length")?;
    for i in 0..chars {
        r.scope(format!("character {}", i + 1), |r| {
            r.take(RICH_CHAR_RECORD)?;
            read_wstr_len(r, "font name length").map(|_| ())
        })?;
    }
    Ok(())
}

/// Reads a WStr (i32 unit count + UTF-16LE) and returns its unit count.
fn read_wstr_len(r: &mut Reader<'_>, field: &'static str) -> Result<usize, ParseError> {
    let at = r.pos();
    let raw = r.i32()?;
    let units = r.count(raw.into(), field, at)?;
    let len = r.byte_len(units, 2, field, at)?;
    r.take(len)?;
    Ok(units)
}

/// §6.2 SafeBMP.
fn read_safebmp(r: &mut Reader<'_>) -> Result<SafeBmp, ParseError> {
    let at = r.pos();
    let header: [u8; 0x20] = r.array()?;
    if &header[..8] != SAFEBMP_MAGIC {
        return Err(r.error(
            at,
            ParseErrorKind::BadMagic {
                expected: "SafeBMP ",
            },
        ));
    }
    let size = i64_at(&header, 0x08);
    let is_path = u32_at(&header, 0x10) != 0;
    let stamp = u64_at(&header, 0x14);
    let mut flags_stamp = [0u8; 0x10];
    flags_stamp.copy_from_slice(&header[0x10..0x20]);

    let payload = if size <= 0 {
        Payload::Empty { size }
    } else {
        let len = r.count(size, "SafeBMP size", at + 0x08)?;
        if is_path && len % 2 != 0 {
            return Err(r.error(
                at + 0x08,
                ParseErrorKind::InvalidLength {
                    field: "SafeBMP path size",
                    value: size,
                },
            ));
        }
        let bytes = r.take(len)?;
        if is_path {
            Payload::Path(
                bytes
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect(),
            )
        } else {
            Payload::Embedded(bytes.to_vec())
        }
    };
    Ok(SafeBmp {
        flags_stamp,
        stamp,
        payload,
    })
}
