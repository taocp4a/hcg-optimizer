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

/// One zip: `new Session(bytes, settingsJson)` reads and plans it; `optimize(onProgress)`
/// returns the output zip.
#[wasm_bindgen]
pub struct Session(session::SessionCore);

#[wasm_bindgen]
impl Session {
    #[wasm_bindgen(constructor)]
    pub fn new(zip: Vec<u8>, settings_json: &str) -> Result<Session, JsError> {
        session::SessionCore::new(zip, settings_json)
            .map(Session)
            .map_err(|e| JsError::new(&e))
    }

    /// The plan preview as JSON (see `hcg_app::report::PlanReport`).
    #[wasm_bindgen(js_name = planJson)]
    pub fn plan_json(&self) -> String {
        self.0.plan_json().to_string()
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
