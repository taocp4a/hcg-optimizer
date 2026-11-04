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
    /// Name without extension (`xxx`). The image folder must have this name.
    pub stem: String,
    /// Where the hcg was found in the input (e.g. `church/xxx.hcg`), for messages.
    pub hcg_path: String,
    /// Where the image folder was found (e.g. `church/xxx`); `None` if there was none.
    pub folder_path: Option<String>,
    pub hcg: Vec<u8>,
    /// Direct children of the image folder, sorted by name.
    pub images: Vec<BundleFile>,
}
