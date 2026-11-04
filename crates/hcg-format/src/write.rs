//! Writer: emits the model in the order it was read. Infallible (contract C2): counts and
//! lengths are derived from the data, and everything else is raw bytes.

use crate::model::{HcgFile, Layer, Object, Page, Payload, SafeBmp};

impl HcgFile {
    /// Serializes the file. For an unmodified file the output equals the parsed input.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.header);
        out.extend_from_slice(&self.doc_header);
        write_layer(&mut out, &self.global_layer);
        push_count(&mut out, self.pages.len());
        for page in &self.pages {
            write_page(&mut out, page);
        }
        out
    }
}

fn write_page(out: &mut Vec<u8>, page: &Page) {
    out.extend_from_slice(&page.header);
    push_count(out, page.layers.len());
    for layer in &page.layers {
        write_layer(out, layer);
    }
    out.extend_from_slice(&page.trailer);
}

fn write_layer(out: &mut Vec<u8>, layer: &Layer) {
    // The header's object count is unchanged: objects cannot be added or removed.
    out.extend_from_slice(&layer.header);
    for object in &layer.objects {
        write_object(out, object);
    }
}

fn write_object(out: &mut Vec<u8>, object: &Object) {
    out.extend_from_slice(&object.head);
    if let Some(image) = &object.image {
        write_safebmp(out, image);
    }
    for image in &object.images {
        write_safebmp(out, image);
    }
    out.extend_from_slice(&object.rest);
}

fn write_safebmp(out: &mut Vec<u8>, bmp: &SafeBmp) {
    out.extend_from_slice(b"SafeBMP ");
    let size = match &bmp.payload {
        Payload::Empty { size } => *size,
        Payload::Path(units) => byte_len_i64(units.len() * 2),
        Payload::Embedded(bytes) => byte_len_i64(bytes.len()),
    };
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&bmp.flags_stamp);
    match &bmp.payload {
        Payload::Empty { .. } => {}
        Payload::Path(units) => units
            .iter()
            .for_each(|u| out.extend_from_slice(&u.to_le_bytes())),
        Payload::Embedded(bytes) => out.extend_from_slice(bytes),
    }
}

/// Counts come from parsed data or bounded edits, so they always fit in an i32.
fn push_count(out: &mut Vec<u8>, n: usize) {
    out.extend_from_slice(&i32::try_from(n).unwrap_or(i32::MAX).to_le_bytes());
}

fn byte_len_i64(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}
