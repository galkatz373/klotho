//! Load and write modular Intent projects on disk.

use std::fs;
use std::path::{Path, PathBuf};

use klotho_ir::{
    Flattened, IntentDoc, IntentModule, IntentProject, ProjectBundle, from_ron, migrate_doc, to_ron,
};

use crate::error::AuthorError;
use crate::parse::{parse_kdown, parse_ron};

/// Loaded authoring input: a legacy document or a modular project.
pub enum Loaded {
    /// Single [`IntentDoc`] (RON or kdown).
    Doc(IntentDoc),
    /// Modular project plus module bodies.
    Project(ProjectBundle),
}

/// Load `path` as an [`IntentDoc`], flattening a project when needed.
pub fn load_file(path: &Path) -> Result<IntentDoc, AuthorError> {
    match load_any(path)? {
        Loaded::Doc(doc) => Ok(doc),
        Loaded::Project(bundle) => Ok(flatten_bundle(&bundle)?.doc),
    }
}

/// Load a document or a project (resolving module paths).
pub fn load_any(path: &Path) -> Result<Loaded, AuthorError> {
    let src = fs::read_to_string(path)
        .map_err(|e| AuthorError::Io(format!("{}: {e}", path.display())))?;
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("kdown") => {
            Ok(Loaded::Doc(parse_kdown(&src).map_err(AuthorError::from)?))
        }
        _ => {
            if let Ok(project) = from_ron::<IntentProject>(&src) {
                let modules = load_modules(path, &project)?;
                Ok(Loaded::Project(ProjectBundle { project, modules }))
            } else {
                Ok(Loaded::Doc(parse_ron(&src).map_err(AuthorError::from)?))
            }
        }
    }
}

/// Flatten a loaded bundle, expanding pattern instances first.
pub fn flatten_bundle(bundle: &ProjectBundle) -> Result<Flattened, AuthorError> {
    let expanded = klotho_pattern::expand_bundle(bundle)?;
    expanded
        .project
        .flatten(&expanded.modules)
        .map_err(AuthorError::from)
}

/// Wrap `doc` as a one-module project and write `project.ron` plus module files.
pub fn migrate_to_dir(
    project: &str,
    module_id: &str,
    doc: IntentDoc,
    out_dir: &Path,
) -> Result<ProjectBundle, AuthorError> {
    let bundle = migrate_doc(project.into(), module_id.into(), doc)?;
    write_bundle(&bundle, out_dir)?;
    Ok(bundle)
}

/// Write a bundle using the paths recorded on the project refs.
pub fn write_bundle(bundle: &ProjectBundle, out_dir: &Path) -> Result<(), AuthorError> {
    fs::create_dir_all(out_dir).map_err(|e| AuthorError::Io(e.to_string()))?;
    let project_text = to_ron(&bundle.project)?;
    fs::write(out_dir.join("project.ron"), project_text)
        .map_err(|e| AuthorError::Io(e.to_string()))?;
    for module_ref in &bundle.project.modules {
        let module = bundle
            .modules
            .iter()
            .find(|m| m.id == module_ref.id)
            .ok_or_else(|| AuthorError::Io(format!("missing module {}", module_ref.id)))?;
        let dest = out_dir.join(&module_ref.path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| AuthorError::Io(e.to_string()))?;
        }
        let text = to_ron(module)?;
        fs::write(&dest, text).map_err(|e| AuthorError::Io(e.to_string()))?;
    }
    Ok(())
}

fn load_modules(
    project_path: &Path,
    project: &IntentProject,
) -> Result<Vec<IntentModule>, AuthorError> {
    let root = project_path.parent().unwrap_or_else(|| Path::new("."));
    let mut modules = Vec::with_capacity(project.modules.len());
    for module_ref in &project.modules {
        let path = resolve_module_path(root, &module_ref.path)?;
        let src = fs::read_to_string(&path)
            .map_err(|e| AuthorError::Io(format!("{}: {e}", path.display())))?;
        let module: IntentModule = from_ron(&src)?;
        modules.push(module);
    }
    Ok(modules)
}

fn resolve_module_path(root: &Path, rel: &str) -> Result<PathBuf, AuthorError> {
    if rel.is_empty() || Path::new(rel).is_absolute() || rel.split(['/', '\\']).any(|p| p == "..") {
        return Err(AuthorError::Io(format!("illegal module path {rel}")));
    }
    Ok(root.join(rel.split('/').collect::<PathBuf>()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use klotho_core::{Hash, LocusKind};
    use klotho_ir::{Name, ProvenanceId, SeedFact, StyleIntent};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn chair() -> IntentDoc {
        IntentDoc {
            style: StyleIntent::default(),
            canon_diffs: Vec::new(),
            seed: vec![SeedFact::Locus {
                name: Name::from("chair"),
                kind: LocusKind::Relic,
            }],
            minds: Vec::new(),
            provenance: ProvenanceId(Hash::ZERO),
        }
    }

    fn scratch() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("klotho-kai02-{nanos}"))
    }

    #[test]
    fn migrate_round_trip_on_disk() {
        let dir = scratch();
        let bundle = migrate_to_dir("hearth", "main", chair(), &dir).unwrap();
        let loaded = load_any(&dir.join("project.ron")).unwrap();
        match loaded {
            Loaded::Project(again) => {
                assert_eq!(again.project, bundle.project);
                assert_eq!(flatten_bundle(&again).unwrap().doc, chair());
            }
            Loaded::Doc(_) => panic!("expected project"),
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parent_path_is_rejected() {
        let err = resolve_module_path(Path::new("/tmp"), "../secret.ron").unwrap_err();
        assert!(err.to_string().contains("illegal module path"));
    }
}
