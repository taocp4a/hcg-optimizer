//! Ports: capabilities the use cases need from the outside world. Implementations live in
//! `hcg-adapters` or in each frontend.
//!
//! E.g. the default implementation uses the pure-Rust `image` crate; the web frontend may plug in
//! the browser's Canvas encoder if needed.

/// Image file formats the use cases tell apart. Detected from the file's leading signature
/// bytes, not its name; only TGA (which has no signature) is recognised by its extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    // ez-CGER's image-open dialog accepts exactly these six.
    Jpeg,
    Png,
    Bmp,
    Gif,
    Dds,
    Tga,
    // Not accepted by ez-CGER.
    WebP,
    Heic,
    /// Anything else (corrupt data, other formats).
    Other,
}

impl ImageFormat {
    /// Formats ez-CGER can load (its image-open dialog lists exactly these).
    pub fn supported_by_app(self) -> bool {
        matches!(
            self,
            Self::Jpeg | Self::Png | Self::Bmp | Self::Gif | Self::Dds | Self::Tga
        )
    }

    /// Formats this tool shrinks. GIF (may be animated), DDS (GPU texture) and TGA (rare) are
    /// left as they are.
    pub fn resizable(self) -> bool {
        matches!(self, Self::Jpeg | Self::Png | Self::Bmp)
    }
}

/// Header-level facts about an image file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageProbe {
    pub format: ImageFormat,
    /// Width and height, if the codec can read this format's header.
    pub dimensions: Option<(u32, u32)>,
}

/// Output image format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Jpeg { quality: u8 },
    Png,
}

/// Image processing port. Split into steps so a large photo is decoded and downscaled once and
/// only the small result is re-encoded while searching for a JPEG quality.
pub trait ImageCodec {
    /// Decoded, downscaled pixels. Opaque to the use cases.
    type Image;
    type Error: std::error::Error + Send + Sync + 'static;

    /// Reads only what is needed to know the format and size. `file_name` is used only to
    /// recognise TGA, which has no signature.
    fn probe(&self, bytes: &[u8], file_name: &str) -> Result<ImageProbe, Self::Error>;

    /// Decodes and, if larger, downscales to fit within `max_w`×`max_h` keeping the aspect ratio.
    /// Never upscales. Pixels stay in stored orientation; the orientation tag is kept for
    /// [`ImageCodec::encode`].
    fn decode_fit(&self, bytes: &[u8], max_w: u32, max_h: u32) -> Result<Self::Image, Self::Error>;

    fn dimensions(&self, image: &Self::Image) -> (u32, u32);

    /// True if any pixel has alpha < 255.
    fn uses_alpha(&self, image: &Self::Image) -> bool;

    fn encode(&self, image: &Self::Image, format: OutputFormat) -> Result<Vec<u8>, Self::Error>;
}

/// Progress reporting port. The CLI prints to the terminal; the web frontend updates the page.
pub trait Progress {
    /// `done`/`total` steps and a description of the current task.
    fn report(&mut self, done: usize, total: usize, message: &str);

    /// True if the user cancelled. Use cases check this between steps and stop.
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// Progress sink that does nothing.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoProgress;

impl Progress for NoProgress {
    fn report(&mut self, _done: usize, _total: usize, _message: &str) {}
}
