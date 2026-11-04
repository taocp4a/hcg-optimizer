//! Browser bindings. All processing happens inside the browser; files never leave the machine.

use wasm_bindgen::prelude::*;

/// Version of the built core, shown in the page footer.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Computes the image-folder file name (`ID + file name`). `stamp` is passed as a JS BigInt.
#[wasm_bindgen(js_name = subfolderFileName)]
pub fn subfolder_file_name(path: &str, stamp: u64) -> String {
    hcg_format::subfolder_file_name(path, stamp)
}

// TODO: `optimize(zip: &[u8]) -> Result<Vec<u8>, JsError>` — wire up the hcg-app use cases.
