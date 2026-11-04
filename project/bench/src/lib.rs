//! The full pipeline as the frontends will run it: zip in, zip out. Each stage is timed.

use hcg_adapters::archive::{ZipLimits, read_zip, write_zip};
use hcg_adapters::image_codec::PureRustCodec;
use hcg_app::policy::ResizePolicy;
use hcg_app::ports::NoProgress;
use hcg_app::run::{execute, plan_run};
use wasm_bindgen::prelude::*;

/// Runs the pipeline on `zip` (consumed, so it can be freed after reading) and returns a
/// one-line summary. `now` returns milliseconds.
/// Linear memory in MB on wasm32 (it only grows, so this is the high-water mark so far).
fn mem() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        format!(" [{} MB]", core::arch::wasm32::memory_size(0) * 65536 / 1_048_576)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        String::new()
    }
}

pub fn pipeline(zip: Vec<u8>, now: &dyn Fn() -> f64) -> String {
    let policy = ResizePolicy::default();
    let codec = PureRustCodec::default();
    let input_len = zip.len();
    let m0 = mem();
    let t0 = now();
    let files = read_zip(&zip, &ZipLimits::default()).expect("read zip");
    drop(zip);
    let m1 = mem();
    let t1 = now();
    let plan = plan_run(files, &codec, &policy);
    let shrink = plan.shrink_count();
    let m2 = mem();
    let t2 = now();
    let result = execute(plan, &codec, &policy, &mut NoProgress).expect("not cancelled");
    let m3 = mem();
    let t3 = now();
    let out = result.package(&policy);
    let output = write_zip(out).expect("write zip");
    let m4 = mem();
    let t4 = now();
    format!(
        "input {:.1} MB{m0} | read {:.0} ms{m1} | plan {:.0} ms ({shrink} to shrink){m2} | \
         apply {:.0} ms{m3} | package+zip {:.0} ms{m4} | total {:.0} ms | output {:.1} MB",
        input_len as f64 / 1_048_576.0,
        t1 - t0,
        t2 - t1,
        t3 - t2,
        t4 - t3,
        t4 - t0,
        output.len() as f64 / 1_048_576.0
    )
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = performance)]
    fn now() -> f64;
}

#[wasm_bindgen]
pub fn run_wasm(zip: Vec<u8>) -> String {
    pipeline(zip, &|| now())
}
