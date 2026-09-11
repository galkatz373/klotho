//! Explicit pattern-version migrations. No implicit upstream upgrades.

use klotho_ir::{PatternArg, PatternInstance};

use crate::def::PatternSpec;
use crate::error::PatternError;
use crate::expand::bind_args;
use crate::stdlib::{lookup, specs};

/// Bind `instance` at `to` by copying args and filling new defaults.
pub fn migrate_instance(
    instance: &PatternInstance,
    to: u32,
) -> Result<PatternInstance, PatternError> {
    let from = lookup(instance.pattern.as_str(), instance.version).ok_or_else(|| {
        PatternError::Version {
            id: instance.pattern.0.clone(),
            requested: instance.version,
        }
    })?;
    let dest = lookup(instance.pattern.as_str(), to).ok_or_else(|| PatternError::Version {
        id: instance.pattern.0.clone(),
        requested: to,
    })?;
    if dest.version == from.version {
        return Ok(instance.clone());
    }
    if !migrates_from(dest, from.version) {
        return Err(PatternError::Version {
            id: instance.pattern.0.clone(),
            requested: to,
        });
    }
    let bound = bind_args(from, instance)?;
    let mut args = Vec::new();
    for param in dest.params {
        if let Some(value) = bound.get(param.name) {
            args.push(PatternArg {
                key: klotho_ir::Name::from(param.name),
                value: value.clone(),
            });
        } else if let Some(default) = param.default {
            args.push(PatternArg {
                key: klotho_ir::Name::from(param.name),
                value: default.to_value(),
            });
        } else {
            return Err(PatternError::Arg {
                id: dest.id.to_owned(),
                key: param.name.to_owned(),
                reason: "migration missing required arg".into(),
            });
        }
    }
    Ok(PatternInstance {
        anchor: instance.anchor,
        module: instance.module,
        instance: instance.instance.clone(),
        pattern: instance.pattern.clone(),
        version: dest.version,
        args,
    })
}

fn migrates_from(dest: &PatternSpec, from: u32) -> bool {
    if dest.from_version == Some(from) {
        return true;
    }
    // Identity: same id, dest is reachable by a chain of from_version edges.
    let mut cur = dest.from_version;
    let mut guard = 0;
    while let Some(v) = cur {
        if v == from {
            return true;
        }
        let prev = specs().iter().find(|s| s.id == dest.id && s.version == v);
        cur = prev.and_then(|s| s.from_version);
        guard += 1;
        if guard > 8 {
            break;
        }
    }
    false
}

/// Journey names before and after a one-step upgrade.
#[must_use]
pub fn migration_journeys(
    id: &str,
    from: u32,
    to: u32,
) -> Option<(&'static [&'static str], &'static [&'static str])> {
    let src = lookup(id, from)?;
    let dst = lookup(id, to)?;
    Some((src.journeys, dst.journeys))
}
