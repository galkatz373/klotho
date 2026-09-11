//! Distaff CLI: cook, preview, migrate, and flatten Intent documents.

#![forbid(unsafe_code)]

use std::env;
use std::path::Path;
use std::process::ExitCode;

use klotho_author::{
    Loaded, cook_summary, cook_validated, flatten_bundle, load_any, load_doc_file, load_file,
    migrate_to_dir, preview_summary,
};
use klotho_ir::to_ron;

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
    match cmd.as_str() {
        "cook" | "preview" => {
            let path = args.next().ok_or_else(usage)?;
            if args.next().is_some() {
                return Err(usage());
            }
            let doc = load_file(Path::new(&path)).map_err(|e| e.to_string())?;
            let cooked = cook_validated(&doc).map_err(|e| e.to_string())?;
            match cmd.as_str() {
                "cook" => Ok(cook_summary(&cooked)),
                "preview" => Ok(preview_summary(&cooked)),
                _ => Err(usage()),
            }
        }
        "flatten" => {
            let path = args.next().ok_or_else(usage)?;
            if args.next().is_some() {
                return Err(usage());
            }
            match load_any(Path::new(&path)).map_err(|e| e.to_string())? {
                Loaded::Project(bundle) => {
                    let flat = flatten_bundle(&bundle).map_err(|e| e.to_string())?;
                    to_ron(&flat.doc).map_err(|e| e.to_string())
                }
                Loaded::Doc(doc) => to_ron(&doc).map_err(|e| e.to_string()),
            }
        }
        "migrate" => {
            let path = args.next().ok_or_else(usage)?;
            let out = args.next().ok_or_else(usage)?;
            let mut project = None;
            let mut module = None;
            while let Some(flag) = args.next() {
                match flag.as_str() {
                    "--project" => {
                        project = Some(args.next().ok_or_else(usage)?);
                    }
                    "--module" => {
                        module = Some(args.next().ok_or_else(usage)?);
                    }
                    _ => return Err(usage()),
                }
            }
            let src = Path::new(&path);
            let doc = load_doc_file(src).map_err(|e| e.to_string())?;
            let stem = src
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("project");
            let project = project.unwrap_or_else(|| stem.to_owned());
            let module = module.unwrap_or_else(|| "main".to_owned());
            migrate_to_dir(&project, &module, doc, Path::new(&out)).map_err(|e| e.to_string())?;
            Ok(format!("migrated {path} -> {out}/project.ron\n"))
        }
        _ => Err(usage()),
    }
}

fn usage() -> String {
    "usage: distaff <cook|preview|flatten> <file>\n       distaff migrate <file> <out-dir> [--project NAME] [--module NAME]".into()
}
