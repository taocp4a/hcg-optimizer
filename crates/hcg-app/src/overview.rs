//! Page overview (issue #19): every page of an hcg as a contact sheet built from the page
//! thumbnails the app stores in the file. This module reads the thumbnails ([`read_pages`]) and
//! lays the sheets out ([`layout`]); an adapter draws them (`hcg-adapters::pdf`). The two are
//! separate so a frontend can keep prepared thumbnails and lay them out again with other
//! settings after the hcg itself is gone (issue #21). No image folder is needed.
//!
//! Units are PDF points (1/72 inch) with the origin at the **top-left** of the sheet.

use crate::bundle::Bundle;
use hcg_format::{HcgFile, ParseError, parse};

/// A4 landscape, in points.
pub const A4_LANDSCAPE: (f32, f32) = (841.89, 595.28);
const MARGIN: f32 = 28.0;
/// Space for the header line above the grid.
const HEADER: f32 = 22.0;
/// Space for the page number under each thumbnail.
const LABEL: f32 = 14.0;
const GAP: f32 = 10.0;
/// Frame aspect when the first page reports no usable size (every sample is 1920×1080).
const DEFAULT_ASPECT: f32 = 16.0 / 9.0;

/// How many thumbnails go on one sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PerSheet {
    /// 3 columns × 2 rows.
    Six,
    /// 4 columns × 3 rows.
    #[default]
    Twelve,
    /// 5 columns × 4 rows.
    Twenty,
}

impl PerSheet {
    /// Every choice, smallest first.
    pub const ALL: [PerSheet; 3] = [PerSheet::Six, PerSheet::Twelve, PerSheet::Twenty];

    /// Thumbnails per sheet.
    pub fn count(self) -> usize {
        let (cols, rows) = self.grid();
        cols * rows
    }

    /// (columns, rows) on a landscape sheet.
    pub fn grid(self) -> (usize, usize) {
        match self {
            PerSheet::Six => (3, 2),
            PerSheet::Twelve => (4, 3),
            PerSheet::Twenty => (5, 4),
        }
    }

    /// The choice with exactly `count` thumbnails per sheet.
    pub fn from_count(count: usize) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.count() == count)
    }
}

/// Overview options chosen by the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OverviewSettings {
    pub per_sheet: PerSheet,
}

/// A rectangle in points, origin top-left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// One thumbnail slot.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    /// 1-based page number, as the app shows it.
    pub page: usize,
    /// Where the thumbnail goes; the drawer fits it inside, keeping its own aspect ratio.
    pub frame: Rect,
    /// Text under the frame (the page number).
    pub label: String,
    /// Baseline of the label, horizontally centred on `x`.
    pub label_x: f32,
    pub label_y: f32,
}

/// One sheet (one PDF page).
#[derive(Debug, Clone, PartialEq)]
pub struct Sheet {
    /// Header text, printable ASCII only (PDF base fonts have no Hangul).
    pub header: String,
    /// Header baseline, left-aligned.
    pub header_x: f32,
    pub header_y: f32,
    pub cells: Vec<Cell>,
}

/// The laid-out sheets of one hcg (no image data).
#[derive(Debug, Clone, PartialEq)]
pub struct Overview {
    /// The hcg stem (may contain any characters); for document metadata.
    pub title: String,
    /// Suggested output file name: `<stem>_pages.pdf`.
    pub file_name: String,
    /// Sheet size in points (width, height).
    pub paper: (f32, f32),
    pub page_count: usize,
    pub sheets: Vec<Sheet>,
}

/// The pages of one hcg as the overview needs them. Thumbnails stay inside the parsed file
/// (nothing copied); get them with [`Pages::thumbnail`].
#[derive(Debug)]
pub struct Pages<'a> {
    /// The hcg stem.
    pub title: String,
    pub page_count: usize,
    /// Width / height of the first page (frame shape on the sheets).
    pub aspect: f32,
    file: HcgFile<'a>,
}

impl Pages<'_> {
    /// The thumbnail BMP of a 1-based page number, as stored by the app.
    pub fn thumbnail(&self, page: usize) -> Option<&[u8]> {
        self.file.thumbnail(page.checked_sub(1)?)
    }
}

/// Suggested file name of the overview of a bundle.
pub fn overview_file_name(stem: &str) -> String {
    format!("{stem}_pages.pdf")
}

/// Reads the pages of `bundle`'s hcg. Fails only if the hcg cannot be read.
pub fn read_pages(bundle: &Bundle) -> Result<Pages<'_>, ParseError> {
    let file = parse(&bundle.hcg)?;
    let aspect = file
        .pages()
        .next()
        .filter(|p| p.width > 0 && p.height > 0)
        .map_or(DEFAULT_ASPECT, |p| p.width as f32 / p.height as f32);
    Ok(Pages {
        title: bundle.stem.clone(),
        page_count: file.page_count(),
        aspect,
        file,
    })
}

/// Lays out `page_count` pages of frame shape `aspect` (width / height) for the hcg `title`.
pub fn layout(
    title: &str,
    page_count: usize,
    aspect: f32,
    settings: &OverviewSettings,
) -> Overview {
    let aspect = if aspect.is_finite() && aspect > 0.0 {
        aspect
    } else {
        DEFAULT_ASPECT
    };
    let paper = A4_LANDSCAPE;
    let title_suffix = ascii_title(title)
        .map(|t| format!("  -  {t}"))
        .unwrap_or_default();
    let per_sheet = settings.per_sheet.count();
    let cells = grid(paper, settings.per_sheet, aspect);
    let sheets = if page_count == 0 {
        vec![Sheet {
            header: format!("No pages{title_suffix}"),
            header_x: MARGIN,
            header_y: MARGIN + HEADER / 2.0,
            cells: Vec::new(),
        }]
    } else {
        (0..page_count)
            .step_by(per_sheet)
            .map(|first| {
                let last = (first + per_sheet).min(page_count);
                let range = if last == first + 1 {
                    format!("Page {last}")
                } else {
                    format!("Pages {}-{last}", first + 1)
                };
                Sheet {
                    header: format!("{range} of {page_count}{title_suffix}"),
                    header_x: MARGIN,
                    header_y: MARGIN + HEADER / 2.0,
                    cells: (first..last)
                        .zip(&cells)
                        .map(|(index, &(frame, label_x, label_y))| Cell {
                            page: index + 1,
                            frame,
                            label: (index + 1).to_string(),
                            label_x,
                            label_y,
                        })
                        .collect(),
                }
            })
            .collect()
    };
    Overview {
        title: title.to_string(),
        file_name: overview_file_name(title),
        paper,
        page_count,
        sheets,
    }
}

/// The stem for the visible header, only if it is printable ASCII.
fn ascii_title(stem: &str) -> Option<&str> {
    (!stem.is_empty() && stem.bytes().all(|b| (0x20..0x7F).contains(&b))).then_some(stem)
}

/// Frames (row by row) and label positions of one sheet: the largest frames of `aspect` that
/// fit, the grid centred horizontally right under the header.
fn grid(paper: (f32, f32), per_sheet: PerSheet, aspect: f32) -> Vec<(Rect, f32, f32)> {
    let (cols, rows) = per_sheet.grid();
    let (colsf, rowsf) = (cols as f32, rows as f32);
    let avail_w = paper.0 - 2.0 * MARGIN;
    let avail_h = paper.1 - 2.0 * MARGIN - HEADER;
    let width_by_w = (avail_w - (colsf - 1.0) * GAP) / colsf;
    let height_by_h = (avail_h - (rowsf - 1.0) * GAP - rowsf * LABEL) / rowsf;
    let height = height_by_h.min(width_by_w / aspect);
    let width = height * aspect;
    let grid_w = colsf * width + (colsf - 1.0) * GAP;
    let x0 = MARGIN + (avail_w - grid_w) / 2.0;
    let y0 = MARGIN + HEADER;
    (0..rows)
        .flat_map(|r| (0..cols).map(move |c| (r, c)))
        .map(|(r, c)| {
            let x = x0 + c as f32 * (width + GAP);
            let y = y0 + r as f32 * (height + LABEL + GAP);
            let frame = Rect {
                x,
                y,
                width,
                height,
            };
            (frame, x + width / 2.0, y + height + LABEL - 3.0)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_sheet_choices() {
        assert_eq!(
            PerSheet::ALL.map(PerSheet::count),
            [6, 12, 20],
            "documented choices"
        );
        assert_eq!(PerSheet::default(), PerSheet::Twelve);
        assert_eq!(PerSheet::from_count(20), Some(PerSheet::Twenty));
        assert_eq!(PerSheet::from_count(9), None);
    }

    #[test]
    fn frames_fit_the_paper_without_overlap() {
        for per_sheet in PerSheet::ALL {
            for aspect in [16.0 / 9.0, 4.0 / 3.0, 1.0] {
                let cells = grid(A4_LANDSCAPE, per_sheet, aspect);
                assert_eq!(cells.len(), per_sheet.count());
                for (i, (a, _, label_y)) in cells.iter().enumerate() {
                    assert!((a.width / a.height - aspect).abs() < 1e-3);
                    assert!(a.x >= MARGIN - 1e-3 && a.y >= MARGIN + HEADER - 1e-3);
                    assert!(a.x + a.width <= A4_LANDSCAPE.0 - MARGIN + 1e-3);
                    assert!(*label_y <= A4_LANDSCAPE.1 - MARGIN + 1e-3);
                    for (b, _, _) in &cells[i + 1..] {
                        let apart = a.x + a.width <= b.x
                            || b.x + b.width <= a.x
                            || a.y + a.height + LABEL <= b.y
                            || b.y + b.height + LABEL <= a.y;
                        assert!(apart, "{per_sheet:?} {aspect}: {a:?} overlaps {b:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn ascii_title_only() {
        assert_eq!(ascii_title("Sunday 3"), Some("Sunday 3"));
        assert_eq!(ascii_title("주일 3부"), None);
        assert_eq!(ascii_title(""), None);
    }
}
