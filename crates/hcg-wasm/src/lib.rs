//! Browser bindings (issue #17). The page's Web Worker creates a [`Session`] per zip. All
//! processing happens inside the browser; files never leave the machine.
//!
//! The logic is in [`session::SessionCore`] (plain Rust, tested natively); this file only
//! converts between JavaScript and Rust.

pub mod session;

use hcg_app::ports::Progress;
use wasm_bindgen::prelude::*;

/// Version of the built core, shown in the page footer.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// One file: `new Session(bytes, fileName, settingsJson)` reads and plans a zip (or a bare
/// `.hcg`); `overviewPdf(index, json)` draws a page overview; `optimize(onProgress)` returns the
/// output zip.
#[wasm_bindgen]
pub struct Session(session::SessionCore);

#[wasm_bindgen]
impl Session {
    #[wasm_bindgen(constructor)]
    pub fn new(bytes: Vec<u8>, file_name: &str, settings_json: &str) -> Result<Session, JsError> {
        session::SessionCore::open(bytes, file_name, settings_json)
            .map(Session)
            .map_err(|e| JsError::new(&e))
    }

    /// The plan preview as JSON (see `hcg_app::report::PlanReport`).
    #[wasm_bindgen(js_name = planJson)]
    pub fn plan_json(&self) -> String {
        self.0.plan_json().to_string()
    }

    /// The page overview PDF of the `index`-th hcg; `settings_json` like `{"per_sheet":12}`.
    #[wasm_bindgen(js_name = overviewPdf)]
    pub fn overview_pdf(&mut self, index: usize, settings_json: &str) -> Result<Vec<u8>, JsError> {
        self.0
            .overview_pdf(index, settings_json)
            .map_err(|e| JsError::new(&e))
    }

    /// Shrinks images; calls `on_progress(done, total, fileName)` before each image.
    pub fn optimize(&mut self, on_progress: &js_sys::Function) -> Result<Vec<u8>, JsError> {
        let mut progress = JsProgress(on_progress);
        self.0.optimize(&mut progress).map_err(|e| JsError::new(&e))
    }

    /// The reports as JSON (see `hcg_app::report::Report`), after `optimize`.
    #[wasm_bindgen(js_name = reportsJson)]
    pub fn reports_json(&self) -> String {
        self.0.reports_json().to_string()
    }
}

struct JsProgress<'a>(&'a js_sys::Function);

impl Progress for JsProgress<'_> {
    fn report(&mut self, done: usize, total: usize, message: &str) {
        let _ = self.0.call3(
            &JsValue::NULL,
            &JsValue::from(done as u32),
            &JsValue::from(total as u32),
            &JsValue::from_str(message),
        );
    }
}
