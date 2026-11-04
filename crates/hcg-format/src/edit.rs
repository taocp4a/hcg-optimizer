//! The public read/edit surface used by the application layer. The only mutation offered is
//! replacing an image path (domain rule 1); every other byte stays as parsed.

use crate::error::EditError;
use crate::image_id::subfolder_file_name_utf16;
use crate::model::{HcgFile, Layer, Payload, SafeBmp};
use crate::reader::i32_at;

/// Longest accepted path, in UTF-16 units (Windows extended-length path limit).
pub const MAX_PATH_UNITS: usize = 32_767;

/// Opaque handle to one SafeBMP in a file. Obtained from [`HcgFile::images`]; cannot be built
/// outside this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImageSlot {
    page: Option<usize>,
    layer: usize,
    object: usize,
    role: ImageRole,
}

/// Which SafeBMP of an object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageRole {
    /// The object's own image (sub.hasImage).
    Primary,
    /// Entry `n` (0-based) of the object's image list / sequence (sub.nImages).
    Sequence(usize),
}

/// What a SafeBMP holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageSource {
    /// Reference to an external file (lossy conversion if the UTF-16 is malformed).
    Path(String),
    /// Image bytes stored inside the hcg (read-only; never created by this tool).
    Embedded { len: usize },
    /// No image data.
    Empty,
}

/// One image reference, as listed by [`HcgFile::images`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRef {
    pub slot: ImageSlot,
    /// 0-based page index; `None` for the global Background layer.
    pub page: Option<usize>,
    /// 0-based layer index within the page (always 0 for the global layer).
    pub layer: usize,
    /// 0-based object index within the layer.
    pub object: usize,
    pub role: ImageRole,
    /// §6.2 original file modification time.
    pub stamp: u64,
    pub source: ImageSource,
    /// File name inside the image folder (`ID + file name`, §6.3). `None` unless `source` is a path.
    pub folder_file_name: Option<String>,
}

/// Transition kind (§5 page header +0x28).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionKind {
    Cut,
    Mix,
    LeftToRight,
    RightToLeft,
    BottomToTop,
    TopToBottom,
    Unknown(i32),
}

/// Page transition setting (§5 page header +0x24..+0x2C).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageTransition {
    /// The page has no own setting; the app's global effect is used.
    Global,
    /// The page's own effect.
    Effect {
        kind: TransitionKind,
        duration_ms: i32,
    },
}

/// Facts about one page needed by the plan and report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageInfo {
    /// 0-based page index.
    pub index: usize,
    pub width: i32,
    pub height: i32,
    pub transition: PageTransition,
    /// Number of objects on the page, over all its layers.
    pub object_count: usize,
}

impl HcgFile {
    /// Number of pages.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Number of objects in the global Background layer.
    pub fn global_object_count(&self) -> usize {
        self.global_layer.objects.len()
    }

    /// Per-page facts, in page order.
    pub fn pages(&self) -> impl Iterator<Item = PageInfo> + '_ {
        self.pages.iter().enumerate().map(|(index, page)| {
            let h = &page.header;
            let transition = if i32_at(h, 0x24) == 0 {
                PageTransition::Global
            } else {
                let kind = match i32_at(h, 0x28) {
                    0 => TransitionKind::Cut,
                    1 => TransitionKind::Mix,
                    2 => TransitionKind::LeftToRight,
                    3 => TransitionKind::RightToLeft,
                    4 => TransitionKind::BottomToTop,
                    5 => TransitionKind::TopToBottom,
                    other => TransitionKind::Unknown(other),
                };
                PageTransition::Effect {
                    kind,
                    duration_ms: i32_at(h, 0x2C),
                }
            };
            PageInfo {
                index,
                width: i32_at(h, 0x0C),
                height: i32_at(h, 0x10),
                transition,
                object_count: page.layers.iter().map(|l| l.objects.len()).sum(),
            }
        })
    }

    /// Every SafeBMP in file order: global layer first, then pages; within an object the
    /// primary image, then the image list.
    pub fn images(&self) -> Vec<ImageRef> {
        let mut out = Vec::new();
        collect_layer(&mut out, None, 0, &self.global_layer);
        for (p, page) in self.pages.iter().enumerate() {
            for (l, layer) in page.layers.iter().enumerate() {
                collect_layer(&mut out, Some(p), l, layer);
            }
        }
        out
    }

    /// Replaces the path of a path-type SafeBMP. Only that block's size field and payload
    /// change in the output (contract C5). The stamp is kept, so the folder ID follows the new
    /// path.
    pub fn set_image_path(&mut self, slot: ImageSlot, new_path: &str) -> Result<(), EditError> {
        let units: Vec<u16> = new_path.encode_utf16().collect();
        if units.is_empty() {
            return Err(EditError::EmptyPath);
        }
        if units.len() > MAX_PATH_UNITS {
            return Err(EditError::PathTooLong(units.len()));
        }
        let bmp = self.safebmp_mut(slot).ok_or(EditError::InvalidSlot)?;
        match &mut bmp.payload {
            Payload::Path(path) => {
                *path = units;
                Ok(())
            }
            Payload::Embedded(_) | Payload::Empty { .. } => Err(EditError::NotAPath),
        }
    }

    fn safebmp_mut(&mut self, slot: ImageSlot) -> Option<&mut SafeBmp> {
        let layer = match slot.page {
            None if slot.layer == 0 => &mut self.global_layer,
            None => return None,
            Some(p) => self.pages.get_mut(p)?.layers.get_mut(slot.layer)?,
        };
        let object = layer.objects.get_mut(slot.object)?;
        match slot.role {
            ImageRole::Primary => object.image.as_mut(),
            ImageRole::Sequence(i) => object.images.get_mut(i),
        }
    }
}

fn collect_layer(out: &mut Vec<ImageRef>, page: Option<usize>, layer: usize, l: &Layer) {
    for (object, obj) in l.objects.iter().enumerate() {
        let primary = obj.image.iter().map(|b| (ImageRole::Primary, b));
        let sequence = obj
            .images
            .iter()
            .enumerate()
            .map(|(i, b)| (ImageRole::Sequence(i), b));
        for (role, bmp) in primary.chain(sequence) {
            let (source, folder_file_name) = match &bmp.payload {
                Payload::Path(units) => (
                    ImageSource::Path(String::from_utf16_lossy(units)),
                    Some(subfolder_file_name_utf16(units, bmp.stamp)),
                ),
                Payload::Embedded(bytes) => (ImageSource::Embedded { len: bytes.len() }, None),
                Payload::Empty { .. } => (ImageSource::Empty, None),
            };
            out.push(ImageRef {
                slot: ImageSlot {
                    page,
                    layer,
                    object,
                    role,
                },
                page,
                layer,
                object,
                role,
                stamp: bmp.stamp,
                source,
                folder_file_name,
            });
        }
    }
}
