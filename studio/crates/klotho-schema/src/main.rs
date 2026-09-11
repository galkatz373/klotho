#![forbid(unsafe_code)]
//! Schema compatibility command used by CI and authoring tools.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let command = env::args().nth(1).unwrap_or_else(|| "check".to_owned());
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let golden = root.join("goldens/schema-v1.json");
    let catalog = klotho_schema::generate(&klotho_canon::Canon::default());
    match command.as_str() {
        "print" => match klotho_schema::to_pretty_json(&catalog) {
            Ok(json) => {
                print!("{json}");
                ExitCode::SUCCESS
            }
            Err(error) => fail(&error.to_string()),
        },
        "check" => match klotho_schema::check_golden(&catalog, &golden) {
            Ok(()) => {
                println!(
                    "KAI schema v{} compatible ({})",
                    catalog.version, catalog.toolchain_hash
                );
                ExitCode::SUCCESS
            }
            Err(error) => fail(&error),
        },
        _ => fail("usage: klotho-schema [check|print]"),
    }
}

fn fail(message: &str) -> ExitCode {
    eprintln!("klotho-schema: {message}");
    ExitCode::FAILURE
}
