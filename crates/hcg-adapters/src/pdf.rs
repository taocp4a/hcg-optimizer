//! Draws a page overview ([`hcg_app::overview::Overview`], issue #19) as a PDF.
//!
//! Each thumbnail BMP is decoded on its own, converted to RGB, deflated and written as an image
//! object right away, so only one decoded thumbnail exists at a time. Text uses the Helvetica
//! base font (ASCII only, nothing embedded); the hcg name goes into the document title, which
//! PDF stores as UTF-16 and so may be Korean.

use flate2::Compression;
use flate2::write::ZlibEncoder;
use hcg_app::overview::{Overview, Rect};
use image::{ImageFormat, ImageReader, Limits};
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Ref, Str, TextStr};
use std::io::{Cursor, Write};

/// Largest thumbnail accepted, per side. The app writes 384×216.
const MAX_THUMBNAIL_SIDE: u32 = 4096;
/// Allocation cap for decoding one thumbnail.
const MAX_THUMBNAIL_ALLOC: u64 = 64 << 20;
const HEADER_SIZE: f32 = 10.0;
const LABEL_SIZE: f32 = 8.0;
const PLACEHOLDER: &str = "no thumbnail";
const FONT: Name<'static> = Name(b"F1");

/// The drawn PDF.
#[derive(Debug)]
pub struct OverviewPdf {
    pub bytes: Vec<u8>,
    /// 1-based pages drawn as an empty "no thumbnail" box: saved without a thumbnail, or one that
    /// could not be decoded.
    pub blank_pages: Vec<usize>,
}

/// Draws `overview`. Never fails: a thumbnail that cannot be used becomes a placeholder box.
pub fn write_overview_pdf(overview: &Overview<'_>) -> OverviewPdf {
    let (paper_w, paper_h) = overview.paper;
    let mut next = 0;
    let mut alloc = || {
        next += 1;
        Ref::new(next)
    };
    let catalog_id = alloc();
    let tree_id = alloc();
    let font_id = alloc();
    let info_id = alloc();

    let mut pdf = Pdf::new();
    pdf.set_binary_marker(b"HCGO");
    pdf.catalog(catalog_id).pages(tree_id);
    pdf.type1_font(font_id).base_font(Name(b"Helvetica"));
    pdf.document_info(info_id)
        .title(TextStr(&overview.title))
        .subject(TextStr("Page overview"))
        .creator(TextStr(concat!(
            "hcg-optimizer ",
            env!("CARGO_PKG_VERSION")
        )));

    let mut page_ids = Vec::with_capacity(overview.sheets.len());
    let mut blank_pages = Vec::new();
    for sheet in &overview.sheets {
        let page_id = alloc();
        let content_id = alloc();
        page_ids.push(page_id);

        let mut content = Content::new();
        text(
            &mut content,
            &sheet.header,
            HEADER_SIZE,
            sheet.header_x,
            paper_h - sheet.header_y,
        );
        let mut images = Vec::new();
        for cell in &sheet.cells {
            let decoded = overview.thumbnail(cell.page).and_then(decode_thumbnail);
            match decoded {
                Some((width, height, deflated)) => {
                    let image_id = alloc();
                    let mut image = pdf.image_xobject(image_id, &deflated);
                    image.filter(Filter::FlateDecode);
                    image.width(width as i32);
                    image.height(height as i32);
                    image.color_space().device_rgb();
                    image.bits_per_component(8);
                    image.finish();
                    let name = format!("Im{}", images.len() + 1);
                    let r = fit(cell.frame, width as f32 / height as f32);
                    let y = paper_h - r.y - r.height;
                    content.save_state();
                    content.transform([r.width, 0.0, 0.0, r.height, r.x, y]);
                    content.x_object(Name(name.as_bytes()));
                    content.restore_state();
                    outline(&mut content, r, paper_h, 0.6);
                    images.push((name, image_id));
                }
                None => {
                    blank_pages.push(cell.page);
                    outline(&mut content, cell.frame, paper_h, 0.75);
                    let f = cell.frame;
                    let x = f.x + (f.width - helvetica_width(PLACEHOLDER, LABEL_SIZE)) / 2.0;
                    let y = paper_h - (f.y + f.height / 2.0) - LABEL_SIZE / 3.0;
                    text(&mut content, PLACEHOLDER, LABEL_SIZE, x, y);
                }
            }
            let x = cell.label_x - helvetica_width(&cell.label, LABEL_SIZE) / 2.0;
            text(
                &mut content,
                &cell.label,
                LABEL_SIZE,
                x,
                paper_h - cell.label_y,
            );
        }

        let mut page = pdf.page(page_id);
        page.media_box(pdf_writer::Rect::new(0.0, 0.0, paper_w, paper_h));
        page.parent(tree_id);
        page.contents(content_id);
        let mut resources = page.resources();
        resources.fonts().pair(FONT, font_id);
        let mut objects = resources.x_objects();
        for (name, id) in &images {
            objects.pair(Name(name.as_bytes()), *id);
        }
        objects.finish();
        resources.finish();
        page.finish();
        pdf.stream(content_id, &content.finish());
    }
    let count = page_ids.len() as i32;
    pdf.pages(tree_id).kids(page_ids).count(count);

    OverviewPdf {
        bytes: pdf.finish(),
        blank_pages,
    }
}

/// Decodes a thumbnail BMP to deflated 8-bit RGB: `(width, height, zlib data)`.
fn decode_thumbnail(bmp: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut reader = ImageReader::with_format(Cursor::new(bmp), ImageFormat::Bmp);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_THUMBNAIL_SIDE);
    limits.max_image_height = Some(MAX_THUMBNAIL_SIDE);
    limits.max_alloc = Some(MAX_THUMBNAIL_ALLOC);
    reader.limits(limits);
    let rgb = reader.decode().ok()?.into_rgb8();
    let (width, height) = rgb.dimensions();
    if width == 0 || height == 0 {
        return None;
    }
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(rgb.as_raw()).ok()?;
    Some((width, height, encoder.finish().ok()?))
}

/// The largest rectangle of `aspect` inside `frame`, centred.
fn fit(frame: Rect, aspect: f32) -> Rect {
    let (mut width, mut height) = (frame.width, frame.width / aspect);
    if height > frame.height {
        height = frame.height;
        width = height * aspect;
    }
    Rect {
        x: frame.x + (frame.width - width) / 2.0,
        y: frame.y + (frame.height - height) / 2.0,
        width,
        height,
    }
}

/// A thin grey outline around `r` (top-left coordinates).
fn outline(content: &mut Content, r: Rect, paper_h: f32, gray: f32) {
    content.save_state();
    content.set_stroke_gray(gray);
    content.set_line_width(0.5);
    content.rect(r.x, paper_h - r.y - r.height, r.width, r.height);
    content.stroke();
    content.restore_state();
}

/// One line of Helvetica text at baseline `(x, y)` (PDF coordinates). Non-ASCII bytes are
/// replaced by `?`, since the base font's standard encoding has no Hangul.
fn text(content: &mut Content, s: &str, size: f32, x: f32, y: f32) {
    let bytes: Vec<u8> = s
        .chars()
        .map(|c| {
            if c.is_ascii() && !c.is_ascii_control() {
                c as u8
            } else {
                b'?'
            }
        })
        .collect();
    content.begin_text();
    content.set_font(FONT, size);
    content.set_fill_gray(0.15);
    content.next_line(x, y);
    content.show(Str(&bytes));
    content.end_text();
}

/// Approximate Helvetica advance width (exact for digits, space and lower case).
fn helvetica_width(s: &str, size: f32) -> f32 {
    let units: u32 = s
        .chars()
        .map(|c| match c {
            ' ' | 't' | 'f' => 278,
            'i' | 'j' | 'l' => 222,
            'm' => 833,
            'w' => 722,
            'r' => 333,
            'c' | 'k' | 's' | 'v' | 'x' | 'y' | 'z' => 500,
            _ => 556,
        })
        .sum();
    units as f32 * size / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_keeps_aspect_and_stays_inside() {
        let frame = Rect {
            x: 10.0,
            y: 20.0,
            width: 160.0,
            height: 90.0,
        };
        let wide = fit(frame, 2.0);
        assert_eq!((wide.width, wide.height), (160.0, 80.0));
        assert_eq!(wide.y, 25.0);
        let tall = fit(frame, 1.0);
        assert_eq!((tall.width, tall.height), (90.0, 90.0));
        assert_eq!(tall.x, 45.0);
    }

    #[test]
    fn digit_widths_are_exact() {
        assert_eq!(helvetica_width("12", 10.0), 11.12);
        assert_eq!(helvetica_width(PLACEHOLDER, 1000.0), 5725.0);
    }

    #[test]
    fn broken_thumbnails_are_rejected() {
        assert!(decode_thumbnail(b"BM").is_none());
        // A header claiming 60000 x 60000 pixels with no pixel data.
        let mut bmp = vec![0u8; 54];
        bmp[..2].copy_from_slice(b"BM");
        bmp[2..6].copy_from_slice(&54u32.to_le_bytes());
        bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
        bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
        bmp[18..22].copy_from_slice(&60_000i32.to_le_bytes());
        bmp[22..26].copy_from_slice(&60_000i32.to_le_bytes());
        bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
        bmp[28..30].copy_from_slice(&32u16.to_le_bytes());
        assert!(decode_thumbnail(&bmp).is_none());
    }
}
