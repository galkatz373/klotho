//! Distaff CLI: cook and preview Intent documents (RON or kdown).

#![forbid(unsafe_code)]

use std::env;
use std::path::Path;
use std::process::ExitCode;

use klotho_author::{cook_summary, cook_validated, load_file, preview_summary};

fn main() -> ExitCode {
    match run() {
        Ok(out) => {
            print!("{out}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<String, String> {
    let mut args = env::args().skip(1);
    let cmd = args.next().ok_or_else(usage)?;
    let path = args.next().ok_or_else(usage)?;
    let doc = load_file(Path::new(&path)).map_err(|e| e.to_string())?;
    let cooked = cook_validated(&doc).map_err(|e| e.to_string())?;
    match cmd.as_str() {
        "cook" => Ok(cook_summary(&cooked)),
        "preview" => Ok(preview_summary(&cooked)),
        _ => Err(usage()),
    }
}

fn usage() -> String {
    "usage: distaff <cook|preview> <file>".into()
}
