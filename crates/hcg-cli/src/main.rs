//! hcg-optimizer CLI (development and testing). Usage: `hcg-optimizer <input.zip> [output.zip]`
//!
//! Frontend layer: argument parsing and file I/O only. Processing logic lives in hcg-app.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(input) = args.first() else {
        eprintln!("usage: hcg-optimizer <input.zip> [output.zip]");
        return ExitCode::from(2);
    };
    // TODO: read zip -> adapters (zip -> Bundle) -> app::analyze/apply -> adapters (Bundle -> zip) -> write
    eprintln!("not implemented yet: {input}");
    ExitCode::FAILURE
}
