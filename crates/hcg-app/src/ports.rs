//! Ports: capabilities the use cases need from the outside world. Implementations live in
//! `hcg-adapters` or in each frontend.
//!
//! E.g. the default implementation uses the pure-Rust `image` crate; the web frontend may plug in
//! the browser's Canvas encoder if needed.

/// Basic facts about a decoded image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageInfo {
    pub width: u32,
    pub height: u32,
    /// Whether the alpha channel actually contains values other than 255.
    pub uses_alpha: bool,
}

/// Output image format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Jpeg { quality: u8 },
    Png,
}

/// Image processing port.
pub trait ImageCodec {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Reads the header/pixels and reports size and alpha usage.
    fn inspect(&self, bytes: &[u8]) -> Result<ImageInfo, Self::Error>;

    /// Shrinks to fit within `max_w`×`max_h` keeping the aspect ratio (unchanged if already
    /// smaller) and encodes as `format`.
    fn resize_encode(
        &self,
        bytes: &[u8],
        max_w: u32,
        max_h: u32,
        format: OutputFormat,
    ) -> Result<Vec<u8>, Self::Error>;
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
