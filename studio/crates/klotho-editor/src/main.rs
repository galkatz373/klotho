//! Headless Distaff editor: cook a RON/kdown path and print outliner + dashboard.

#![forbid(unsafe_code)]

use std::env;
use std::path::Path;
use std::process::ExitCode;

use klotho_editor::EditorSession;

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
    let path = args.next().ok_or_else(usage)?;
    if args.next().is_some() {
        return Err(usage());
    }
    let mut session = EditorSession::load(Path::new(&path)).map_err(|e| e.to_string())?;
    session.cook().map_err(|e| e.to_string())?;
    let mut out = String::new();
    match session.cook_dashboard() {
        Some(dash) => out.push_str(&dash.to_string()),
        None => return Err("no cook".into()),
    }
    out.push_str(&session.outliner().to_string());
    Ok(out)
}

fn usage() -> String {
    "usage: distaff-editor <file>".into()
}
