#![forbid(unsafe_code)]
//! Command-line validation and report emission for the KAI benchmark contract.

use klotho_kai_bench::{default_root, validate_repository, write_report};
use std::{env, path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "validate".into());
    let root = args.next().map_or_else(default_root, PathBuf::from);
    match command.as_str() {
        "validate" => match validate_repository(&root) {
            Ok(summary) => {
                println!(
                    "KAI-00 valid: {} public + {} held-out = {} outcomes; {} planned PRs",
                    summary.public_outcomes,
                    summary.held_out_outcomes,
                    summary.total_outcomes,
                    summary.planned_prs
                );
                ExitCode::SUCCESS
            }
            Err(errors) => {
                for error in errors {
                    eprintln!("KAI-00: {error}");
                }
                ExitCode::FAILURE
            }
        },
        "report" => {
            let output = args.next().map_or_else(
                || root.join("target/kai-benchmark-report.json"),
                PathBuf::from,
            );
            match write_report(&root, &output) {
                Ok(()) => {
                    println!("wrote {}", output.display());
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("KAI-00: {error}");
                    ExitCode::FAILURE
                }
            }
        }
        _ => {
            eprintln!("usage: klotho-kai-bench [validate|report] [ROOT] [REPORT.json]");
            ExitCode::FAILURE
        }
    }
}
