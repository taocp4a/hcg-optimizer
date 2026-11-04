//! Unit of work: one hcg file and its image folder. In-memory, independent of the zip format.

/// One file inside the image folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleFile {
    /// File name inside the folder (`ID + original file name`, e.g. `28BD2D7Emark_1.png`).
    pub name: String,
    pub bytes: Vec<u8>,
}

/// An `xxx.hcg` + `xxx/` pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    /// Name without extension (`xxx`); equal to the folder name.
    pub stem: String,
    pub hcg: Vec<u8>,
    pub images: Vec<BundleFile>,
}
