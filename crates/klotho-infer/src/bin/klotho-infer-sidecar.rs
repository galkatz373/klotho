//! Standalone development entry point for the Klotho inference sidecar.

#![forbid(unsafe_code)]

fn main() -> std::process::ExitCode {
    match klotho_infer::run_sidecar() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("infer sidecar: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
