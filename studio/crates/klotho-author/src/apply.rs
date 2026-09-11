//! Apply semantic edits to a [`ProjectBundle`] by [`AnchorId`].

use std::collections::BTreeMap;

use klotho_core::{Hash, LocusKind, PoseMm};
use klotho_ir::{
    AnchorId, AnchorKind, CanonDiff, IntentModule, IntentModuleRef, LockEntry, ModuleLock, Name,
    NameAlias, ObjectAnchor, PatternArg, PatternInstance, ProjectBundle, Rel, SeedFact, Tombstone,
    module_content_hash, to_ron,
};
use klotho_prove::hash_bytes;
use serde::{Deserialize, Serialize};

use crate::error::AuthorError;

const BUNDLE_HASH_DOMAIN: &[u8] = b"klotho-bundle-v1";

/// Seed fact addressed by immutable identity rather than current [`Name`].
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum AnchoredSeedFact {
    /// Relation row.
    Rel {
        /// Subject.
        a: AnchorId,
        /// Edge.
        rel: Rel,
        /// Object.
        b: AnchorId,
    },
    /// Quantity row.
    Qty {
        /// Locus.
        of: AnchorId,
        /// Resource.
        res: Name,
        /// Value.
        value: i32,
    },
    /// Pose row.
    Pose {
        /// Locus.
        of: AnchorId,
        /// Pose.
        pose: PoseMm,
    },
}

/// One identity-addressed mutation.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum SemanticEdit {
    /// Insert a module and refresh the lock.
    AddModule {
        /// Module body.
        module: IntentModule,
    },
    /// Allocate a locus in `module`.
    AddLocus {
        /// Owning module.
        module: AnchorId,
        /// Frozen identity (already derived).
        anchor: AnchorId,
        /// Current name.
        name: Name,
        /// Packed kind.
        kind: LocusKind,
    },
    /// Insert or replace a seed fact in `module`.
    AddFact {
        /// Module whose seed is written.
        module: AnchorId,
        /// Fact.
        fact: AnchoredSeedFact,
    },
    /// Append a Canon patch in `module`.
    AddCanonDiff {
        /// Owning module.
        module: AnchorId,
        /// Patch.
        diff: CanonDiff,
    },
    /// Tombstone `target` and drop its live rows.
    Remove {
        /// Removed object.
        target: AnchorId,
        /// Non-empty reason.
        reason: String,
    },
    /// Change [`Name`] only; [`AnchorId`] is unchanged.
    Rename {
        /// Live object.
        target: AnchorId,
        /// Replacement name.
        to: Name,
    },
    /// Record a pattern instance. Expansion happens at flatten.
    Instantiate {
        /// Instance payload, including owning module.
        instance: PatternInstance,
    },
    /// Overwrite or insert one pattern argument.
    SetArgument {
        /// Instance identity.
        instance: AnchorId,
        /// Argument name.
        key: Name,
        /// Bound argument.
        value: PatternArg,
    },
}

/// Result of a successful edit.
#[derive(Clone, Eq, PartialEq, Debug, Default)]
pub struct ApplyOutcome {
    /// Anchors that read or write the edited object (impact analysis).
    pub impact: Vec<AnchorId>,
}

/// Apply `edit` in place. Collections are canonicalized so disjoint writes commute.
pub fn apply_edit(
    bundle: &mut ProjectBundle,
    edit: SemanticEdit,
) -> Result<ApplyOutcome, AuthorError> {
    let outcome = apply_edit_unlocked(bundle, edit)?;
    refresh_locks(bundle)?;
    Ok(outcome)
}

/// Apply many edits, refreshing the lock once.
pub fn apply_edits<I>(bundle: &mut ProjectBundle, edits: I) -> Result<ApplyOutcome, AuthorError>
where
    I: IntoIterator<Item = SemanticEdit>,
{
    let mut outcome = ApplyOutcome::default();
    for edit in edits {
        let next = apply_edit_unlocked(bundle, edit)?;
        outcome.impact.extend(next.impact);
    }
    refresh_locks(bundle)?;
    Ok(outcome)
}

/// Apply without refreshing the module lock. Call [`refresh_locks`] after a batch.
pub fn apply_edit_unlocked(
    bundle: &mut ProjectBundle,
    edit: SemanticEdit,
) -> Result<ApplyOutcome, AuthorError> {
    let outcome = match edit {
        SemanticEdit::AddModule { module } => add_module(bundle, module)?,
        SemanticEdit::AddLocus {
            module,
            anchor,
            name,
            kind,
        } => add_locus(bundle, module, anchor, name, kind)?,
        SemanticEdit::AddFact { module, fact } => add_fact(bundle, module, fact)?,
        SemanticEdit::AddCanonDiff { module, diff } => add_canon_diff(bundle, module, diff)?,
        SemanticEdit::Remove { target, reason } => remove_object(bundle, target, reason)?,
        SemanticEdit::Rename { target, to } => rename_object(bundle, target, to)?,
        SemanticEdit::Instantiate { instance } => instantiate(bundle, instance)?,
        SemanticEdit::SetArgument {
            instance,
            key,
            value,
        } => set_argument(bundle, instance, key, value)?,
    };
    Ok(outcome)
}

/// blake3 of the locked project plus each module content hash, domain-separated.
pub fn bundle_content_hash(bundle: &ProjectBundle) -> Result<Hash, AuthorError> {
    let project_text = to_ron(&bundle.project)?;
    let mut buf = Vec::with_capacity(BUNDLE_HASH_DOMAIN.len() + project_text.len() + 32);
    buf.extend_from_slice(BUNDLE_HASH_DOMAIN);
    buf.extend_from_slice(&(project_text.len() as u32).to_le_bytes());
    buf.extend_from_slice(project_text.as_bytes());
    let mut modules: Vec<&IntentModule> = bundle.modules.iter().collect();
    modules.sort_by(|a, b| a.id.cmp(&b.id));
    for module in modules {
        let hash = module_content_hash(module)?;
        buf.extend_from_slice(hash.as_bytes());
    }
    Ok(hash_bytes(&buf))
}

/// Recompute module-ref and lock hashes from current bodies.
pub fn refresh_locks(bundle: &mut ProjectBundle) -> Result<(), AuthorError> {
    let mut by_id: BTreeMap<Name, Hash> = BTreeMap::new();
    for module in &bundle.modules {
        by_id.insert(module.id.clone(), module_content_hash(module)?);
    }
    for module_ref in &mut bundle.project.modules {
        let hash = by_id
            .get(&module_ref.id)
            .copied()
            .ok_or_else(|| AuthorError::ModuleNotFound(module_ref.id.0.clone()))?;
        module_ref.hash = hash;
    }
    bundle
        .project
        .modules
        .sort_by(|a, b| a.id.cmp(&b.id).then(a.path.cmp(&b.path)));
    let mut entries: Vec<LockEntry> = by_id
        .into_iter()
        .map(|(id, hash)| LockEntry { id, hash })
        .collect();
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    bundle.project.lock = ModuleLock { entries };
    Ok(())
}

/// Live object with `id`, if any.
#[must_use]
pub fn lookup_object(bundle: &ProjectBundle, id: AnchorId) -> Option<&ObjectAnchor> {
    bundle
        .modules
        .iter()
        .find_map(|m| m.object_anchors.iter().find(|o| o.anchor == id))
}

/// Module with `id`, if any.
#[must_use]
pub fn lookup_module(bundle: &ProjectBundle, id: AnchorId) -> Option<&IntentModule> {
    bundle.modules.iter().find(|m| m.anchor == id)
}

/// Anchors that currently mention `target` (facts, minds, aliases).
#[must_use]
pub fn dependents_of(bundle: &ProjectBundle, target: AnchorId) -> Vec<AnchorId> {
    let Some(object) = lookup_object(bundle, target) else {
        return Vec::new();
    };
    let name = &object.name;
    let mut out = Vec::new();
    for module in &bundle.modules {
        for fact in &module.body.seed {
            match fact {
                SeedFact::Rel { a, b, .. } => {
                    if a == name {
                        if let Some(id) = name_anchor(module, b) {
                            push_unique(&mut out, id);
                        }
                    }
                    if b == name {
                        if let Some(id) = name_anchor(module, a) {
                            push_unique(&mut out, id);
                        }
                    }
                }
                SeedFact::Qty { of, .. } | SeedFact::Pose { of, .. } if of == name => {
                    push_unique(&mut out, target);
                }
                _ => {}
            }
        }
        for mind in &module.body.minds {
            if &mind.locus == name {
                if let Some(id) = module
                    .object_anchors
                    .iter()
                    .find(|o| o.kind == AnchorKind::Mind && o.name == mind.locus)
                    .map(|o| o.anchor)
                {
                    push_unique(&mut out, id);
                }
            }
        }
        for alias in &module.aliases {
            if alias.target == target {
                push_unique(&mut out, alias.target);
            }
        }
    }
    out
}

fn add_module(
    bundle: &mut ProjectBundle,
    module: IntentModule,
) -> Result<ApplyOutcome, AuthorError> {
    module.validate_local()?;
    if bundle
        .modules
        .iter()
        .any(|m| m.id == module.id || m.anchor == module.anchor)
    {
        return Err(AuthorError::DuplicateAnchor(module.anchor.to_string()));
    }
    if occupied_anchors(bundle).contains(&module.anchor) {
        return Err(AuthorError::DuplicateAnchor(module.anchor.to_string()));
    }
    let hash = module_content_hash(&module)?;
    let path = format!("modules/{}.ron", module.id.as_str());
    bundle.project.modules.push(IntentModuleRef {
        id: module.id.clone(),
        path,
        hash,
    });
    bundle.modules.push(module);
    canonicalize_bundle(bundle);
    Ok(ApplyOutcome::default())
}

fn add_locus(
    bundle: &mut ProjectBundle,
    module_id: AnchorId,
    anchor: AnchorId,
    name: Name,
    kind: LocusKind,
) -> Result<ApplyOutcome, AuthorError> {
    name_checked(&name)?;
    if occupied_anchors(bundle).contains(&anchor) {
        return Err(AuthorError::DuplicateAnchor(anchor.to_string()));
    }
    let idx = module_index(bundle, module_id)?;
    let module = &mut bundle.modules[idx];
    reject_name(module, &name)?;
    module.body.seed.push(SeedFact::Locus {
        name: name.clone(),
        kind,
    });
    module.object_anchors.push(ObjectAnchor {
        kind: AnchorKind::Locus,
        name: name.clone(),
        anchor,
    });
    if !module.exports.iter().any(|e| e == &name) {
        module.exports.push(name);
    }
    canonicalize_module(module);
    module.validate_local()?;
    Ok(ApplyOutcome::default())
}

fn add_fact(
    bundle: &mut ProjectBundle,
    module_id: AnchorId,
    fact: AnchoredSeedFact,
) -> Result<ApplyOutcome, AuthorError> {
    let idx = module_index(bundle, module_id)?;
    let seed_fact = resolve_fact(&bundle.modules[idx], &fact)?;
    let module = &mut bundle.modules[idx];
    upsert_seed(&mut module.body.seed, seed_fact);
    canonicalize_module(module);
    module.validate_local()?;
    Ok(ApplyOutcome {
        impact: fact_anchors(&fact),
    })
}

fn instantiate(
    bundle: &mut ProjectBundle,
    instance: PatternInstance,
) -> Result<ApplyOutcome, AuthorError> {
    name_checked(&instance.instance)?;
    name_checked(&instance.pattern)?;
    if instance.version == 0 {
        return Err(AuthorError::Pattern(
            "pattern version must be non-zero".into(),
        ));
    }
    if occupied_anchors(bundle).contains(&instance.anchor) {
        return Err(AuthorError::DuplicateAnchor(instance.anchor.to_string()));
    }
    let idx = module_index(bundle, instance.module)?;
    let module = &mut bundle.modules[idx];
    reject_name(module, &instance.instance)?;
    module.object_anchors.push(ObjectAnchor {
        kind: AnchorKind::Pattern,
        name: instance.instance.clone(),
        anchor: instance.anchor,
    });
    module.patterns.push(instance);
    canonicalize_module(module);
    module.validate_local()?;
    Ok(ApplyOutcome::default())
}

fn set_argument(
    bundle: &mut ProjectBundle,
    instance: AnchorId,
    key: Name,
    value: PatternArg,
) -> Result<ApplyOutcome, AuthorError> {
    name_checked(&key)?;
    if value.key != key {
        return Err(AuthorError::Pattern(
            "SetArgument key must match PatternArg.key".into(),
        ));
    }
    let idx = owning_module_index(bundle, instance)?;
    let module = &mut bundle.modules[idx];
    let slot = module
        .patterns
        .iter_mut()
        .find(|p| p.anchor == instance)
        .ok_or_else(|| AuthorError::MissingAnchor(instance.to_string()))?;
    if let Some(existing) = slot.args.iter_mut().find(|a| a.key == key) {
        *existing = value;
    } else {
        slot.args.push(value);
    }
    canonicalize_module(module);
    module.validate_local()?;
    Ok(ApplyOutcome::default())
}

fn add_canon_diff(
    bundle: &mut ProjectBundle,
    module_id: AnchorId,
    diff: CanonDiff,
) -> Result<ApplyOutcome, AuthorError> {
    let idx = module_index(bundle, module_id)?;
    let module = &mut bundle.modules[idx];
    if let Some((kind, name)) = live_canon_object(&diff) {
        reject_name(module, &name)?;
        let token = format!("{}:{}", kind_prefix(kind), name.as_str());
        let anchor = module.anchor.child(token.as_bytes());
        if module.object_anchors.iter().any(|o| o.anchor == anchor) {
            return Err(AuthorError::DuplicateAnchor(anchor.to_string()));
        }
        module
            .object_anchors
            .push(ObjectAnchor { kind, name, anchor });
    }
    module.body.canon_diffs.push(diff);
    canonicalize_module(module);
    module.validate_local()?;
    Ok(ApplyOutcome::default())
}

fn remove_object(
    bundle: &mut ProjectBundle,
    target: AnchorId,
    reason: String,
) -> Result<ApplyOutcome, AuthorError> {
    if reason.trim().is_empty() {
        return Err(AuthorError::EmptyReason);
    }
    let impact = dependents_of(bundle, target);
    let idx = owning_module_index(bundle, target)?;
    let object = bundle.modules[idx]
        .object_anchors
        .iter()
        .find(|o| o.anchor == target)
        .cloned()
        .ok_or_else(|| AuthorError::MissingAnchor(target.to_string()))?;
    let module = &mut bundle.modules[idx];
    strip_object(module, &object);
    module.tombstones.push(Tombstone {
        anchor: target,
        name: object.name.clone(),
        reason,
    });
    canonicalize_module(module);
    module.validate_local()?;
    Ok(ApplyOutcome { impact })
}

fn rename_object(
    bundle: &mut ProjectBundle,
    target: AnchorId,
    to: Name,
) -> Result<ApplyOutcome, AuthorError> {
    name_checked(&to)?;
    let idx = owning_module_index(bundle, target)?;
    let object = bundle.modules[idx]
        .object_anchors
        .iter()
        .find(|o| o.anchor == target)
        .cloned()
        .ok_or_else(|| AuthorError::MissingAnchor(target.to_string()))?;
    if object.name == to {
        return Ok(ApplyOutcome::default());
    }
    let module = &mut bundle.modules[idx];
    reject_name(module, &to)?;
    rewrite_names(module, object.kind, &object.name, &to);
    if let Some(slot) = module
        .object_anchors
        .iter_mut()
        .find(|o| o.anchor == target)
    {
        slot.name = to.clone();
    }
    module.aliases.push(NameAlias {
        name: object.name.clone(),
        target,
        until_epoch: 1,
    });
    for export in &mut module.exports {
        if *export == object.name {
            *export = to.clone();
        }
    }
    canonicalize_module(module);
    module.validate_local()?;
    Ok(ApplyOutcome::default())
}

fn strip_object(module: &mut IntentModule, object: &ObjectAnchor) {
    let name = &object.name;
    module.body.seed.retain(|fact| match fact {
        SeedFact::Locus { name: n, .. } => n != name,
        SeedFact::Qty { of, .. } | SeedFact::Pose { of, .. } => of != name,
        SeedFact::Rel { a, b, .. } => a != name && b != name,
    });
    module.body.minds.retain(|m| m.locus != *name);
    module.body.canon_diffs.retain(|d| match canon_name(d) {
        Some((kind, n)) => !(kind == object.kind && n == *name),
        None => true,
    });
    module.patterns.retain(|p| p.anchor != object.anchor);
    module.object_anchors.retain(|o| o.anchor != object.anchor);
    module.exports.retain(|e| e != name);
}

fn rewrite_names(module: &mut IntentModule, kind: AnchorKind, from: &Name, to: &Name) {
    match kind {
        AnchorKind::Locus => {
            for fact in &mut module.body.seed {
                match fact {
                    SeedFact::Locus { name, .. } if name == from => *name = to.clone(),
                    SeedFact::Rel { a, b, .. } => {
                        if a == from {
                            *a = to.clone();
                        }
                        if b == from {
                            *b = to.clone();
                        }
                    }
                    SeedFact::Qty { of, .. } | SeedFact::Pose { of, .. } if of == from => {
                        *of = to.clone();
                    }
                    _ => {}
                }
            }
        }
        AnchorKind::Mind => {
            for mind in &mut module.body.minds {
                if mind.locus == *from {
                    mind.locus = to.clone();
                }
            }
        }
        AnchorKind::Law | AnchorKind::Affordance | AnchorKind::Rite | AnchorKind::Beat => {
            for diff in &mut module.body.canon_diffs {
                rewrite_canon_name(diff, from, to);
            }
        }
        AnchorKind::Pattern => {
            for instance in &mut module.patterns {
                if instance.instance == *from {
                    instance.instance = to.clone();
                }
            }
        }
        AnchorKind::Module => {}
    }
}

fn rewrite_canon_name(diff: &mut CanonDiff, from: &Name, to: &Name) {
    match diff {
        CanonDiff::AddLaw(law) if law.id == *from => law.id = to.clone(),
        CanonDiff::RetractLaw { id, .. } if *id == *from => *id = to.clone(),
        CanonDiff::AddAffordance(aff) if aff.id == *from => aff.id = to.clone(),
        CanonDiff::AddRite(rite) if rite.id == *from => rite.id = to.clone(),
        CanonDiff::RetractRite { id, .. } if *id == *from => *id = to.clone(),
        CanonDiff::AddBeat(beat) if beat.id == *from => beat.id = to.clone(),
        _ => {}
    }
}

fn resolve_fact(module: &IntentModule, fact: &AnchoredSeedFact) -> Result<SeedFact, AuthorError> {
    match fact {
        AnchoredSeedFact::Rel { a, rel, b } => Ok(SeedFact::Rel {
            a: current_name(module, *a)?,
            rel: *rel,
            b: current_name(module, *b)?,
        }),
        AnchoredSeedFact::Qty { of, res, value } => {
            res_checked(res)?;
            Ok(SeedFact::Qty {
                of: current_name(module, *of)?,
                res: res.clone(),
                value: *value,
            })
        }
        AnchoredSeedFact::Pose { of, pose } => Ok(SeedFact::Pose {
            of: current_name(module, *of)?,
            pose: *pose,
        }),
    }
}

fn current_name(module: &IntentModule, id: AnchorId) -> Result<Name, AuthorError> {
    module
        .object_anchors
        .iter()
        .find(|o| o.anchor == id)
        .map(|o| o.name.clone())
        .ok_or_else(|| AuthorError::MissingAnchor(id.to_string()))
}

fn name_anchor(module: &IntentModule, name: &Name) -> Option<AnchorId> {
    module
        .object_anchors
        .iter()
        .find(|o| o.kind == AnchorKind::Locus && o.name == *name)
        .map(|o| o.anchor)
}

fn fact_anchors(fact: &AnchoredSeedFact) -> Vec<AnchorId> {
    match *fact {
        AnchoredSeedFact::Rel { a, b, .. } => vec![a, b],
        AnchoredSeedFact::Qty { of, .. } | AnchoredSeedFact::Pose { of, .. } => vec![of],
    }
}

fn upsert_seed(seed: &mut Vec<SeedFact>, fact: SeedFact) {
    if let Some(slot) = seed.iter_mut().find(|f| seed_identity(f, &fact)) {
        *slot = fact;
    } else {
        seed.push(fact);
    }
}

fn seed_identity(a: &SeedFact, b: &SeedFact) -> bool {
    match (a, b) {
        (SeedFact::Locus { name: x, .. }, SeedFact::Locus { name: y, .. }) => x == y,
        (SeedFact::Pose { of: x, .. }, SeedFact::Pose { of: y, .. }) => x == y,
        (
            SeedFact::Qty {
                of: o1, res: r1, ..
            },
            SeedFact::Qty {
                of: o2, res: r2, ..
            },
        ) => o1 == o2 && r1 == r2,
        (
            SeedFact::Rel {
                a: a1,
                rel: r1,
                b: b1,
            },
            SeedFact::Rel {
                a: a2,
                rel: r2,
                b: b2,
            },
        ) => a1 == a2 && r1 == r2 && b1 == b2,
        _ => false,
    }
}

fn reject_name(module: &IntentModule, name: &Name) -> Result<(), AuthorError> {
    if module.object_anchors.iter().any(|o| o.name == *name) {
        return Err(AuthorError::NameInUse(name.0.clone()));
    }
    if module.tombstones.iter().any(|t| t.name == *name) {
        return Err(AuthorError::TombstoneReuse(name.0.clone()));
    }
    if module.aliases.iter().any(|a| a.name == *name) {
        return Err(AuthorError::AliasCollision(name.0.clone()));
    }
    Ok(())
}

fn occupied_anchors(bundle: &ProjectBundle) -> Vec<AnchorId> {
    let mut ids = Vec::new();
    for module in &bundle.modules {
        ids.push(module.anchor);
        for object in &module.object_anchors {
            ids.push(object.anchor);
        }
        for tomb in &module.tombstones {
            ids.push(tomb.anchor);
        }
    }
    ids.sort();
    ids.dedup();
    ids
}

fn module_index(bundle: &ProjectBundle, id: AnchorId) -> Result<usize, AuthorError> {
    bundle
        .modules
        .iter()
        .position(|m| m.anchor == id)
        .ok_or_else(|| AuthorError::ModuleNotFound(id.to_string()))
}

fn owning_module_index(bundle: &ProjectBundle, id: AnchorId) -> Result<usize, AuthorError> {
    bundle
        .modules
        .iter()
        .position(|m| m.object_anchors.iter().any(|o| o.anchor == id))
        .ok_or_else(|| AuthorError::MissingAnchor(id.to_string()))
}

fn canonicalize_bundle(bundle: &mut ProjectBundle) {
    bundle.modules.sort_by(|a, b| a.id.cmp(&b.id));
    for module in &mut bundle.modules {
        canonicalize_module(module);
    }
}

fn canonicalize_module(module: &mut IntentModule) {
    let mut by_name: BTreeMap<&str, AnchorId> = BTreeMap::new();
    for object in &module.object_anchors {
        if object.kind == AnchorKind::Locus {
            by_name.insert(object.name.as_str(), object.anchor);
        }
    }
    module.body.seed.sort_by(|a, b| {
        seed_key(a, &by_name)
            .cmp(&seed_key(b, &by_name))
            .then_with(|| seed_tie(a).cmp(&seed_tie(b)))
    });
    module
        .object_anchors
        .sort_by(|a, b| a.anchor.cmp(&b.anchor).then(a.kind.cmp(&b.kind)));
    module
        .aliases
        .sort_by(|a, b| a.target.cmp(&b.target).then(a.name.cmp(&b.name)));
    module
        .tombstones
        .sort_by(|a, b| a.anchor.cmp(&b.anchor).then(a.name.cmp(&b.name)));
    module
        .patterns
        .sort_by(|a, b| a.instance.cmp(&b.instance).then(a.anchor.cmp(&b.anchor)));
    module.exports.sort();
    module.exports.dedup();
}

fn seed_key(fact: &SeedFact, by_name: &BTreeMap<&str, AnchorId>) -> (u8, AnchorId, u8, AnchorId) {
    match fact {
        SeedFact::Locus { name, .. } => (
            0,
            *by_name.get(name.as_str()).unwrap_or(&AnchorId::ZERO),
            0,
            AnchorId::ZERO,
        ),
        SeedFact::Rel { a, rel, b } => (
            1,
            *by_name.get(a.as_str()).unwrap_or(&AnchorId::ZERO),
            *rel as u8,
            *by_name.get(b.as_str()).unwrap_or(&AnchorId::ZERO),
        ),
        SeedFact::Qty { of, .. } => (
            2,
            *by_name.get(of.as_str()).unwrap_or(&AnchorId::ZERO),
            0,
            AnchorId::ZERO,
        ),
        SeedFact::Pose { of, .. } => (
            3,
            *by_name.get(of.as_str()).unwrap_or(&AnchorId::ZERO),
            0,
            AnchorId::ZERO,
        ),
    }
}

fn seed_tie(fact: &SeedFact) -> String {
    match fact {
        SeedFact::Qty { res, .. } => res.0.clone(),
        SeedFact::Locus { name, .. } => name.0.clone(),
        _ => String::new(),
    }
}

fn live_canon_object(diff: &CanonDiff) -> Option<(AnchorKind, Name)> {
    match diff {
        CanonDiff::AddLaw(law) => Some((AnchorKind::Law, law.id.clone())),
        CanonDiff::AddAffordance(aff) => Some((AnchorKind::Affordance, aff.id.clone())),
        CanonDiff::AddRite(rite) => Some((AnchorKind::Rite, rite.id.clone())),
        CanonDiff::AddBeat(beat) => Some((AnchorKind::Beat, beat.id.clone())),
        CanonDiff::RetractLaw { .. } | CanonDiff::RetractRite { .. } => None,
    }
}

fn canon_name(diff: &CanonDiff) -> Option<(AnchorKind, Name)> {
    match diff {
        CanonDiff::AddLaw(law) => Some((AnchorKind::Law, law.id.clone())),
        CanonDiff::RetractLaw { id, .. } => Some((AnchorKind::Law, id.clone())),
        CanonDiff::AddAffordance(aff) => Some((AnchorKind::Affordance, aff.id.clone())),
        CanonDiff::AddRite(rite) => Some((AnchorKind::Rite, rite.id.clone())),
        CanonDiff::RetractRite { id, .. } => Some((AnchorKind::Rite, id.clone())),
        CanonDiff::AddBeat(beat) => Some((AnchorKind::Beat, beat.id.clone())),
    }
}

fn kind_prefix(kind: AnchorKind) -> &'static str {
    match kind {
        AnchorKind::Module => "module",
        AnchorKind::Locus => "locus",
        AnchorKind::Law => "law",
        AnchorKind::Affordance => "affordance",
        AnchorKind::Rite => "rite",
        AnchorKind::Beat => "beat",
        AnchorKind::Mind => "mind",
        AnchorKind::Pattern => "pattern",
    }
}

fn name_checked(name: &Name) -> Result<(), AuthorError> {
    Name::new(name.as_str())
        .map(|_| ())
        .map_err(AuthorError::from)
}

fn res_checked(name: &Name) -> Result<(), AuthorError> {
    name_checked(name)
}

fn push_unique(out: &mut Vec<AnchorId>, id: AnchorId) {
    if !out.contains(&id) {
        out.push(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klotho_ir::{IntentDoc, ProvenanceId, StyleIntent, migrate_doc};

    fn name(s: &str) -> Name {
        Name::from(s)
    }

    fn empty_doc() -> IntentDoc {
        IntentDoc {
            style: StyleIntent::default(),
            canon_diffs: Vec::new(),
            seed: Vec::new(),
            minds: Vec::new(),
            provenance: ProvenanceId(Hash::ZERO),
        }
    }

    fn chair() -> IntentDoc {
        IntentDoc {
            seed: vec![SeedFact::Locus {
                name: name("chair"),
                kind: LocusKind::Relic,
            }],
            ..empty_doc()
        }
    }

    fn bundle() -> ProjectBundle {
        migrate_doc(name("hearth"), name("main"), chair()).unwrap()
    }

    fn chair_anchor(b: &ProjectBundle) -> AnchorId {
        b.modules[0].object_anchors[0].anchor
    }

    #[test]
    fn add_locus_then_flatten_is_valid() {
        let mut b = bundle();
        let module = b.modules[0].anchor;
        let anchor = AnchorId::derive(b"hearth", b"change:1").child(b"locus:stool");
        apply_edit(
            &mut b,
            SemanticEdit::AddLocus {
                module,
                anchor,
                name: name("stool"),
                kind: LocusKind::Relic,
            },
        )
        .unwrap();
        let flat = b.project.flatten(&b.modules).unwrap();
        assert!(
            flat.doc
                .seed
                .iter()
                .any(|f| matches!(f, SeedFact::Locus { name, .. } if name.as_str() == "stool"))
        );
        assert!(
            flat.anchors
                .iter()
                .any(|o| o.anchor == anchor && o.name.as_str() == "stool")
        );
    }

    #[test]
    fn rename_keeps_anchor_and_records_alias() {
        let mut b = bundle();
        let target = chair_anchor(&b);
        apply_edit(
            &mut b,
            SemanticEdit::Rename {
                target,
                to: name("brass_chair"),
            },
        )
        .unwrap();
        let object = lookup_object(&b, target).unwrap();
        assert_eq!(object.name.as_str(), "brass_chair");
        assert_eq!(object.anchor, target);
        assert!(
            b.modules[0]
                .aliases
                .iter()
                .any(|a| a.name.as_str() == "chair" && a.target == target)
        );
        let flat = b.project.flatten(&b.modules).unwrap();
        assert!(
            flat.doc.seed.iter().any(
                |f| matches!(f, SeedFact::Locus { name, .. } if name.as_str() == "brass_chair")
            )
        );
    }

    #[test]
    fn remove_writes_tombstone_and_rejects_reuse() {
        let mut b = bundle();
        let target = chair_anchor(&b);
        let out = apply_edit(
            &mut b,
            SemanticEdit::Remove {
                target,
                reason: "retired".into(),
            },
        )
        .unwrap();
        assert!(lookup_object(&b, target).is_none());
        assert!(
            b.modules[0]
                .tombstones
                .iter()
                .any(|t| t.anchor == target && t.name.as_str() == "chair")
        );
        let _ = out;
        let module = b.modules[0].anchor;
        let err = apply_edit(
            &mut b,
            SemanticEdit::AddLocus {
                module,
                anchor: AnchorId::derive(b"hearth", b"change:2").child(b"locus:reuse"),
                name: name("chair"),
                kind: LocusKind::Relic,
            },
        )
        .unwrap_err();
        assert!(matches!(err, AuthorError::TombstoneReuse(n) if n == "chair"));
    }

    #[test]
    fn alias_cannot_collide_with_live_name() {
        let mut b = bundle();
        let module = b.modules[0].anchor;
        let stool = AnchorId::derive(b"hearth", b"change:3").child(b"locus:stool");
        apply_edit(
            &mut b,
            SemanticEdit::AddLocus {
                module,
                anchor: stool,
                name: name("stool"),
                kind: LocusKind::Relic,
            },
        )
        .unwrap();
        apply_edit(
            &mut b,
            SemanticEdit::Rename {
                target: stool,
                to: name("brass_stool"),
            },
        )
        .unwrap();
        let chair = chair_anchor(&b);
        let err = apply_edit(
            &mut b,
            SemanticEdit::Rename {
                target: chair,
                to: name("stool"),
            },
        )
        .unwrap_err();
        assert!(matches!(err, AuthorError::AliasCollision(n) if n == "stool"));
    }

    #[test]
    fn disjoint_qty_writes_commute() {
        let mut base = bundle();
        let module = base.modules[0].anchor;
        let a = AnchorId::derive(b"hearth", b"c").child(b"l:a");
        let c = AnchorId::derive(b"hearth", b"c").child(b"l:c");
        apply_edit(
            &mut base,
            SemanticEdit::AddLocus {
                module,
                anchor: a,
                name: name("alpha"),
                kind: LocusKind::Relic,
            },
        )
        .unwrap();
        apply_edit(
            &mut base,
            SemanticEdit::AddLocus {
                module,
                anchor: c,
                name: name("gamma"),
                kind: LocusKind::Relic,
            },
        )
        .unwrap();
        let fact_a = SemanticEdit::AddFact {
            module,
            fact: AnchoredSeedFact::Qty {
                of: a,
                res: name("mass_g"),
                value: 1,
            },
        };
        let fact_c = SemanticEdit::AddFact {
            module,
            fact: AnchoredSeedFact::Qty {
                of: c,
                res: name("mass_g"),
                value: 2,
            },
        };
        let mut left = base.clone();
        apply_edit(&mut left, fact_a.clone()).unwrap();
        apply_edit(&mut left, fact_c.clone()).unwrap();
        let mut right = base;
        apply_edit(&mut right, fact_c).unwrap();
        apply_edit(&mut right, fact_a).unwrap();
        assert_eq!(
            bundle_content_hash(&left).unwrap(),
            bundle_content_hash(&right).unwrap()
        );
        assert_eq!(
            left.project.flatten(&left.modules).unwrap().doc,
            right.project.flatten(&right.modules).unwrap().doc
        );
    }
}
