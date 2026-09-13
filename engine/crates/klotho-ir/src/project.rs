//! Modular Intent: project, modules, lock, anchors, flatten.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use klotho_core::Hash;
use klotho_prove::ProvenanceId;
use klotho_prove::hash_bytes;

use crate::anchor::AnchorId;
use crate::decl::CanonDiff;
use crate::doc::IntentDoc;
use crate::error::IrError;
use crate::name::Name;
use crate::parse::to_ron;
use crate::seed::SeedFact;
use crate::style::StyleIntent;

/// Domain tag mixed into every module content hash.
const MODULE_HASH_DOMAIN: &[u8] = b"klotho-module-v1";

/// Kind of a named authoring object. Frozen discriminants.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[repr(u8)]
pub enum AnchorKind {
    /// The module itself.
    Module = 0,
    /// Seed locus.
    Locus = 1,
    /// Law id.
    Law = 2,
    /// Affordance id.
    Affordance = 3,
    /// Rite id.
    Rite = 4,
    /// Beat id.
    Beat = 5,
    /// Mind spec (actor locus).
    Mind = 6,
    /// Pattern instance. Expanded before flatten; never present at runtime.
    Pattern = 7,
}

impl AnchorKind {
    /// Packed discriminant.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    fn token_prefix(self) -> &'static str {
        match self {
            Self::Module => "module",
            Self::Locus => "locus",
            Self::Law => "law",
            Self::Affordance => "affordance",
            Self::Rite => "rite",
            Self::Beat => "beat",
            Self::Mind => "mind",
            Self::Pattern => "pattern",
        }
    }
}

/// Kind of a [`SourceSpan`] record.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum SpanKind {
    /// `style` block.
    Style,
    /// One [`CanonDiff`].
    CanonDiff,
    /// One [`SeedFact`].
    Seed,
    /// One mind spec.
    Mind,
    /// One parameter declaration.
    Parameter,
    /// One import.
    Import,
    /// One export name.
    Export,
    /// One [`PatternInstance`].
    Pattern,
}

/// Stable span back to a module item. Offsets are over a per-kind canonical stream.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSpan {
    /// Module that authored the item.
    pub module: AnchorId,
    /// Item family.
    pub kind: SpanKind,
    /// Index within that family in the module.
    pub index: u32,
    /// Inclusive start byte in the kind's canonical stream.
    pub start: u32,
    /// Exclusive end byte.
    pub end: u32,
}

/// Immutable identity for one named object inside a module.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectAnchor {
    /// Object family.
    pub kind: AnchorKind,
    /// Current name. Rename updates this; [`Self::anchor`] does not change.
    pub name: Name,
    /// Frozen identity.
    pub anchor: AnchorId,
}

/// Previous name kept for one migration epoch. Cannot be reused as a live name.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NameAlias {
    /// Former name.
    pub name: Name,
    /// Object the alias still refers to.
    pub target: AnchorId,
    /// Exclusive upper bound on the migration epoch that still accepts the alias.
    pub until_epoch: u32,
}

/// Removed object. The name cannot be reused until dependents migrate.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tombstone {
    /// Identity of the removed object.
    pub anchor: AnchorId,
    /// Name at removal time.
    pub name: Name,
    /// Why it was removed.
    pub reason: String,
}

/// Closed parameter type for a module.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum ParameterType {
    /// Authoring name.
    Name,
    /// Signed 32-bit scalar.
    I32,
    /// Boolean.
    Bool,
    /// Semantic anchor.
    Anchor,
}

/// Bound or default parameter value.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum ParameterValue {
    /// [`ParameterType::Name`].
    Name(Name),
    /// [`ParameterType::I32`].
    I32(i32),
    /// [`ParameterType::Bool`].
    Bool(bool),
    /// [`ParameterType::Anchor`].
    Anchor(AnchorId),
}

/// One module parameter. Flatten requires a default until pattern instantiation.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterDecl {
    /// Parameter name.
    pub name: Name,
    /// Value type.
    pub ty: ParameterType,
    /// Default used when the module is flattened without instantiation.
    pub default: Option<ParameterValue>,
}

/// Bound argument on a [`PatternInstance`].
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatternArg {
    /// Parameter name.
    pub key: Name,
    /// Bound value.
    pub value: ParameterValue,
}

/// Parameterized pattern instance. Authoring source; expansion is derived.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatternInstance {
    /// Frozen instance identity.
    pub anchor: AnchorId,
    /// Owning module identity.
    pub module: AnchorId,
    /// Authoring name. Not identity.
    pub instance: Name,
    /// Standard-library pattern id (`traversal.door_key`).
    pub pattern: Name,
    /// Pattern version. Child anchors mix this in.
    pub version: u32,
    /// Bound arguments. Unknown keys fail closed at expansion.
    pub args: Vec<PatternArg>,
}

/// Import of another module, locked by content hash.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleImport {
    /// Imported module id.
    pub id: Name,
    /// Expected content hash of that module.
    pub hash: Hash,
}

/// On-disk pointer to a module plus the locked hash.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentModuleRef {
    /// Module id.
    pub id: Name,
    /// Path relative to the project file. POSIX separators.
    pub path: String,
    /// Content hash of the module bytes.
    pub hash: Hash,
}

/// One lockfile row.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockEntry {
    /// Module id.
    pub id: Name,
    /// Content hash.
    pub hash: Hash,
}

/// Content-hash lock over every module in the project.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleLock {
    /// Sorted `(id, hash)` pairs. Flatten sorts before comparing.
    pub entries: Vec<LockEntry>,
}

/// One Intent module. Flattened to an [`IntentDoc`] before Canon cook.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentModule {
    /// Immutable module identity.
    pub anchor: AnchorId,
    /// Authoring id (`"hearth"`). Not identity.
    pub id: Name,
    /// Module format version.
    pub version: u32,
    /// Acyclic imports, each hash-locked.
    pub imports: Vec<ModuleImport>,
    /// Names this module offers to importers.
    pub exports: Vec<Name>,
    /// Parameters. Flatten fails if any lack a default.
    pub parameters: Vec<ParameterDecl>,
    /// Former names still resolving to [`ObjectAnchor::anchor`].
    pub aliases: Vec<NameAlias>,
    /// Removed objects. Names cannot be reused.
    pub tombstones: Vec<Tombstone>,
    /// Frozen identities for live named objects.
    pub object_anchors: Vec<ObjectAnchor>,
    /// Pattern instances. Flatten fails until they are expanded.
    #[serde(default)]
    pub patterns: Vec<PatternInstance>,
    /// Ordinary Intent body. Flatten concatenates these.
    pub body: IntentDoc,
}

/// Project of modules plus the content-hash lock.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentProject {
    /// Project namespace used when deriving module anchors.
    pub project: Name,
    /// Module refs. Flatten sorts by id; input order has no effect.
    pub modules: Vec<IntentModuleRef>,
    /// Authoritative content hashes.
    pub lock: ModuleLock,
}

/// Migrated or loaded project plus module bodies.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ProjectBundle {
    /// Project manifest and lock.
    pub project: IntentProject,
    /// Module bodies, not necessarily sorted.
    pub modules: Vec<IntentModule>,
}

/// Flattened Intent plus span and identity tables that are not part of the doc.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct Flattened {
    /// Deterministic [`IntentDoc`]. Cook and Trace consume only this.
    pub doc: IntentDoc,
    /// Source spans back to originating modules.
    pub spans: Vec<SourceSpan>,
    /// Live object identities after flatten, sorted by `(kind, name)`.
    pub anchors: Vec<ObjectAnchor>,
}

impl ParameterValue {
    fn ty(&self) -> ParameterType {
        match self {
            Self::Name(_) => ParameterType::Name,
            Self::I32(_) => ParameterType::I32,
            Self::Bool(_) => ParameterType::Bool,
            Self::Anchor(_) => ParameterType::Anchor,
        }
    }
}

impl IntentModule {
    /// blake3 of canonical module RON, domain-separated.
    pub fn content_hash(&self) -> Result<Hash, IrError> {
        module_content_hash(self)
    }

    /// Structural checks that do not require sibling modules.
    pub fn validate_local(&self) -> Result<(), IrError> {
        self.id.check()?;
        self.body.validate()?;
        if self.version == 0 {
            return Err(IrError::InvalidModuleVersion);
        }
        for import in &self.imports {
            import.id.check()?;
        }
        for export in &self.exports {
            export.check()?;
        }
        let live = live_objects(self)?;
        let live_names: BTreeSet<&str> = live.iter().map(|(_, n)| n.as_str()).collect();
        for export in &self.exports {
            if !live_names.contains(export.as_str()) {
                return Err(IrError::ExportUnknown(export.0.clone()));
            }
        }
        for param in &self.parameters {
            param.name.check()?;
            if let Some(value) = &param.default {
                if value.ty() != param.ty {
                    return Err(IrError::ParameterTypeMismatch(param.name.0.clone()));
                }
                if let ParameterValue::Name(n) = value {
                    n.check()?;
                }
            }
        }
        let mut seen_params = BTreeSet::new();
        for param in &self.parameters {
            if !seen_params.insert(param.name.as_str()) {
                return Err(IrError::DuplicateObjectName(param.name.0.clone()));
            }
        }
        let mut seen_patterns = BTreeSet::new();
        for instance in &self.patterns {
            instance.instance.check()?;
            instance.pattern.check()?;
            if instance.version == 0 {
                return Err(IrError::InvalidModuleVersion);
            }
            if instance.module != AnchorId::ZERO && instance.module != self.anchor {
                return Err(IrError::MissingAnchor(instance.instance.0.clone()));
            }
            if !seen_patterns.insert(instance.instance.as_str()) {
                return Err(IrError::DuplicateObjectName(instance.instance.0.clone()));
            }
            let mut keys = BTreeSet::new();
            for arg in &instance.args {
                arg.key.check()?;
                if !keys.insert(arg.key.as_str()) {
                    return Err(IrError::DuplicateObjectName(arg.key.0.clone()));
                }
                if let ParameterValue::Name(n) = &arg.value {
                    n.check()?;
                }
            }
        }
        validate_identities(self, &live, &live_names)?;
        Ok(())
    }
}

impl IntentProject {
    /// Flatten `modules` to a single [`IntentDoc`].
    ///
    /// Module list order, lock entry order, and import-graph order do not
    /// affect the result. Import cycles and hash drift fail closed.
    pub fn flatten(&self, modules: &[IntentModule]) -> Result<Flattened, IrError> {
        flatten_project(self, modules)
    }
}

/// Wrap an existing [`IntentDoc`] as a one-module project.
///
/// Flattening the result yields `doc` unchanged. Child anchors are derived
/// from `(module_anchor, kind, original_name)` at migrate time and then frozen.
pub fn migrate_doc(
    project: Name,
    module_id: Name,
    doc: IntentDoc,
) -> Result<ProjectBundle, IrError> {
    project.check()?;
    module_id.check()?;
    doc.validate()?;
    let module_anchor = AnchorId::derive(
        project.as_str().as_bytes(),
        format!("module:{}", module_id.as_str()).as_bytes(),
    );
    let object_anchors = assign_anchors(&doc, module_anchor)?;
    let mut exports: Vec<Name> = object_anchors.iter().map(|a| a.name.clone()).collect();
    exports.sort();
    exports.dedup();
    let module = IntentModule {
        anchor: module_anchor,
        id: module_id.clone(),
        version: 1,
        imports: Vec::new(),
        exports,
        parameters: Vec::new(),
        aliases: Vec::new(),
        tombstones: Vec::new(),
        object_anchors,
        patterns: Vec::new(),
        body: doc,
    };
    module.validate_local()?;
    let hash = module.content_hash()?;
    let path = format!("modules/{}.ron", module_id.as_str());
    let project = IntentProject {
        project,
        modules: vec![IntentModuleRef {
            id: module_id.clone(),
            path,
            hash,
        }],
        lock: ModuleLock {
            entries: vec![LockEntry {
                id: module_id,
                hash,
            }],
        },
    };
    Ok(ProjectBundle {
        project,
        modules: vec![module],
    })
}

/// Content hash used by the module lock.
pub fn module_content_hash(module: &IntentModule) -> Result<Hash, IrError> {
    let text = to_ron(module)?;
    let mut buf = Vec::with_capacity(MODULE_HASH_DOMAIN.len() + 4 + text.len());
    buf.extend_from_slice(MODULE_HASH_DOMAIN);
    buf.extend_from_slice(&(text.len() as u32).to_le_bytes());
    buf.extend_from_slice(text.as_bytes());
    Ok(hash_bytes(&buf))
}

fn flatten_project(
    project: &IntentProject,
    modules: &[IntentModule],
) -> Result<Flattened, IrError> {
    project.project.check()?;
    let loaded = index_modules(modules)?;
    let refs = unique_refs(&project.modules)?;
    for id in loaded.keys() {
        if !refs.contains_key(*id) {
            return Err(IrError::DuplicateModule((*id).to_owned()));
        }
    }
    for id in refs.keys() {
        if !loaded.contains_key(*id) {
            return Err(IrError::MissingModule((*id).to_owned()));
        }
    }
    for (id, module_ref) in &refs {
        let module = loaded[id];
        module.validate_local()?;
        if let Some(instance) = module.patterns.first() {
            return Err(IrError::UnexpandedPattern(instance.pattern.0.clone()));
        }
        let actual = module.content_hash()?;
        if actual != module_ref.hash {
            return Err(hash_drift(id, module_ref.hash, actual));
        }
        let locked = lock_hash(&project.lock, id)?;
        if actual != locked {
            return Err(hash_drift(id, locked, actual));
        }
    }
    if project.lock.entries.len() != refs.len() {
        return Err(IrError::HashDrift {
            id: project.project.0.clone(),
            expected: refs.len().to_string(),
            actual: project.lock.entries.len().to_string(),
        });
    }
    detect_cycles(&loaded)?;
    for import in loaded.values().flat_map(|m| m.imports.iter()) {
        let Some(target) = loaded.get(import.id.as_str()) else {
            return Err(IrError::MissingModule(import.id.0.clone()));
        };
        let actual = target.content_hash()?;
        if actual != import.hash {
            return Err(hash_drift(import.id.as_str(), import.hash, actual));
        }
    }

    let mut order: Vec<&IntentModule> = refs.values().map(|r| loaded[r.id.as_str()]).collect();
    order.sort_by(|a, b| a.id.cmp(&b.id));

    let mut style = StyleIntent::default();
    let mut canon_diffs = Vec::new();
    let mut seed = Vec::new();
    let mut minds = Vec::new();
    let mut provenances = Vec::new();
    let mut spans = Vec::new();
    let mut anchors = Vec::new();
    let mut seen_live: BTreeSet<(AnchorKind, String)> = BTreeSet::new();

    for module in order {
        for param in &module.parameters {
            if param.default.is_none() {
                return Err(IrError::UnboundParameter(param.name.0.clone()));
            }
        }
        merge_style(&mut style, &module.body.style);
        push_kind_spans(
            &mut spans,
            module.anchor,
            SpanKind::CanonDiff,
            &module.body.canon_diffs,
        )?;
        push_kind_spans(&mut spans, module.anchor, SpanKind::Seed, &module.body.seed)?;
        push_kind_spans(
            &mut spans,
            module.anchor,
            SpanKind::Mind,
            &module.body.minds,
        )?;
        canon_diffs.extend(module.body.canon_diffs.iter().cloned());
        seed.extend(module.body.seed.iter().cloned());
        minds.extend(module.body.minds.iter().cloned());
        provenances.push(module.body.provenance);
        for object in &module.object_anchors {
            let key = (object.kind, object.name.0.clone());
            if !seen_live.insert(key.clone()) {
                return Err(IrError::DuplicateObjectName(key.1));
            }
            anchors.push(object.clone());
        }
    }

    anchors.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.name.cmp(&b.name)));
    let provenance = merge_provenance(&provenances);
    let doc = IntentDoc {
        style,
        canon_diffs,
        seed,
        minds,
        provenance,
    };
    doc.validate()?;
    Ok(Flattened {
        doc,
        spans,
        anchors,
    })
}

fn index_modules(modules: &[IntentModule]) -> Result<BTreeMap<&str, &IntentModule>, IrError> {
    let mut loaded = BTreeMap::new();
    for module in modules {
        module.id.check()?;
        if loaded.insert(module.id.as_str(), module).is_some() {
            return Err(IrError::DuplicateModule(module.id.0.clone()));
        }
    }
    Ok(loaded)
}

fn unique_refs(refs: &[IntentModuleRef]) -> Result<BTreeMap<&str, &IntentModuleRef>, IrError> {
    let mut out = BTreeMap::new();
    for module_ref in refs {
        module_ref.id.check()?;
        if module_ref.path.is_empty() {
            return Err(IrError::EmptyName);
        }
        if out.insert(module_ref.id.as_str(), module_ref).is_some() {
            return Err(IrError::DuplicateModule(module_ref.id.0.clone()));
        }
    }
    Ok(out)
}

fn lock_hash(lock: &ModuleLock, id: &str) -> Result<Hash, IrError> {
    let mut matches = lock
        .entries
        .iter()
        .filter(|e| e.id.as_str() == id)
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(if matches.is_empty() {
            IrError::MissingModule(id.to_owned())
        } else {
            IrError::DuplicateModule(id.to_owned())
        });
    }
    Ok(matches.remove(0).hash)
}

fn hash_drift(id: &str, expected: Hash, actual: Hash) -> IrError {
    IrError::HashDrift {
        id: id.to_owned(),
        expected: expected.to_string(),
        actual: actual.to_string(),
    }
}

fn detect_cycles(loaded: &BTreeMap<&str, &IntentModule>) -> Result<(), IrError> {
    let mut incoming: BTreeMap<&str, usize> = BTreeMap::new();
    let mut outgoing: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for id in loaded.keys() {
        incoming.insert(*id, 0);
        outgoing.insert(*id, Vec::new());
    }
    for module in loaded.values() {
        let mut seen = BTreeSet::new();
        for import in &module.imports {
            if !seen.insert(import.id.as_str()) {
                continue;
            }
            if !loaded.contains_key(import.id.as_str()) {
                return Err(IrError::MissingModule(import.id.0.clone()));
            }
            outgoing
                .get_mut(module.id.as_str())
                .expect("indexed")
                .push(import.id.as_str());
            *incoming.get_mut(import.id.as_str()).expect("indexed") += 1;
        }
    }
    let mut ready: Vec<&str> = incoming
        .iter()
        .filter(|&(_, n)| *n == 0)
        .map(|(id, _)| *id)
        .collect();
    ready.sort_unstable();
    let mut visited = 0usize;
    while let Some(id) = ready.pop() {
        visited += 1;
        let mut nxt = outgoing[id].clone();
        nxt.sort_unstable();
        for dep in nxt {
            let count = incoming.get_mut(dep).expect("indexed");
            *count -= 1;
            if *count == 0 {
                ready.push(dep);
                ready.sort_unstable();
            }
        }
    }
    if visited != loaded.len() {
        let mut cycle: Vec<String> = incoming
            .iter()
            .filter(|&(_, n)| *n > 0)
            .map(|(id, _)| (*id).to_owned())
            .collect();
        cycle.sort();
        return Err(IrError::ImportCycle(cycle));
    }
    Ok(())
}

fn merge_style(dst: &mut StyleIntent, src: &StyleIntent) {
    if !src.notes.is_empty() {
        if dst.notes.is_empty() {
            dst.notes.clone_from(&src.notes);
        } else {
            dst.notes.push('\n');
            dst.notes.push_str(&src.notes);
        }
    }
    dst.palettes.extend(src.palettes.iter().cloned());
    dst.kitbash_tags.extend(src.kitbash_tags.iter().cloned());
}

fn merge_provenance(ids: &[ProvenanceId]) -> ProvenanceId {
    if ids.len() <= 1 {
        return ids.first().copied().unwrap_or(ProvenanceId(Hash::ZERO));
    }
    let mut buf = Vec::with_capacity(ids.len() * 32);
    for id in ids {
        buf.extend_from_slice(id.0.as_bytes());
    }
    ProvenanceId(hash_bytes(&buf))
}

fn push_kind_spans<T: Serialize>(
    spans: &mut Vec<SourceSpan>,
    module: AnchorId,
    kind: SpanKind,
    items: &[T],
) -> Result<(), IrError> {
    let mut offset = 0u32;
    for (index, item) in items.iter().enumerate() {
        let bytes = to_ron(item)?;
        let len = u32::try_from(bytes.len()).unwrap_or(u32::MAX);
        spans.push(SourceSpan {
            module,
            kind,
            index: u32::try_from(index).unwrap_or(u32::MAX),
            start: offset,
            end: offset.saturating_add(len),
        });
        offset = offset.saturating_add(len);
    }
    Ok(())
}

fn live_objects(module: &IntentModule) -> Result<BTreeSet<(AnchorKind, Name)>, IrError> {
    let mut names = BTreeSet::new();
    for (kind, name) in named_objects_with_patterns(module) {
        if !names.insert((kind, name.clone())) {
            return Err(IrError::DuplicateObjectName(name.0));
        }
    }
    Ok(names)
}

fn named_objects(doc: &IntentDoc) -> Vec<(AnchorKind, Name)> {
    let mut out = Vec::new();
    for fact in &doc.seed {
        if let SeedFact::Locus { name, .. } = fact {
            out.push((AnchorKind::Locus, name.clone()));
        }
    }
    for diff in &doc.canon_diffs {
        match diff {
            CanonDiff::AddLaw(law) => out.push((AnchorKind::Law, law.id.clone())),
            CanonDiff::AddAffordance(aff) => out.push((AnchorKind::Affordance, aff.id.clone())),
            CanonDiff::AddRite(rite) => out.push((AnchorKind::Rite, rite.id.clone())),
            CanonDiff::AddBeat(beat) => out.push((AnchorKind::Beat, beat.id.clone())),
            CanonDiff::RetractLaw { id, .. } => out.push((AnchorKind::Law, id.clone())),
            CanonDiff::RetractRite { id, .. } => out.push((AnchorKind::Rite, id.clone())),
        }
    }
    for mind in &doc.minds {
        out.push((AnchorKind::Mind, mind.locus.clone()));
    }
    out
}

fn named_objects_with_patterns(module: &IntentModule) -> Vec<(AnchorKind, Name)> {
    let mut out = named_objects(&module.body);
    for instance in &module.patterns {
        out.push((AnchorKind::Pattern, instance.instance.clone()));
    }
    out
}

fn assign_anchors(doc: &IntentDoc, module: AnchorId) -> Result<Vec<ObjectAnchor>, IrError> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for (kind, name) in named_objects(doc) {
        if !seen.insert((kind, name.clone())) {
            continue;
        }
        name.check()?;
        let token = format!("{}:{}", kind.token_prefix(), name.as_str());
        out.push(ObjectAnchor {
            kind,
            name,
            anchor: module.child(token.as_bytes()),
        });
    }
    out.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.name.cmp(&b.name)));
    Ok(out)
}

fn validate_identities(
    module: &IntentModule,
    live: &BTreeSet<(AnchorKind, Name)>,
    live_names: &BTreeSet<&str>,
) -> Result<(), IrError> {
    let mut by_key: BTreeMap<(AnchorKind, &str), &ObjectAnchor> = BTreeMap::new();
    let mut by_anchor: BTreeSet<AnchorId> = BTreeSet::new();
    if !by_anchor.insert(module.anchor) {
        return Err(IrError::DuplicateAnchor(module.anchor.to_string()));
    }
    for object in &module.object_anchors {
        object.name.check()?;
        if !live.contains(&(object.kind, object.name.clone())) {
            return Err(IrError::MissingAnchor(object.name.0.clone()));
        }
        if by_key
            .insert((object.kind, object.name.as_str()), object)
            .is_some()
        {
            return Err(IrError::DuplicateObjectName(object.name.0.clone()));
        }
        if !by_anchor.insert(object.anchor) {
            return Err(IrError::DuplicateAnchor(object.anchor.to_string()));
        }
    }
    for (kind, name) in live {
        if !by_key.contains_key(&(*kind, name.as_str())) {
            return Err(IrError::MissingAnchor(name.0.clone()));
        }
    }
    let tombstone_names: BTreeSet<&str> =
        module.tombstones.iter().map(|t| t.name.as_str()).collect();
    for tomb in &module.tombstones {
        tomb.name.check()?;
        if tomb.reason.is_empty() {
            return Err(IrError::EmptyName);
        }
        if live_names.contains(tomb.name.as_str()) {
            return Err(IrError::TombstoneReuse(tomb.name.0.clone()));
        }
        if !by_anchor.insert(tomb.anchor) {
            return Err(IrError::DuplicateAnchor(tomb.anchor.to_string()));
        }
    }
    for alias in &module.aliases {
        alias.name.check()?;
        if live_names.contains(alias.name.as_str()) || tombstone_names.contains(alias.name.as_str())
        {
            return Err(IrError::AliasCollision(alias.name.0.clone()));
        }
        if !by_anchor.contains(&alias.target) {
            return Err(IrError::MissingAnchor(alias.name.0.clone()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use klotho_core::LocusKind;

    use crate::from_ron;
    use crate::mind::MindSpec;
    use crate::seed::SeedFact;

    fn name(s: &str) -> Name {
        Name::from(s)
    }

    fn locus(n: &str) -> SeedFact {
        SeedFact::Locus {
            name: name(n),
            kind: LocusKind::Relic,
        }
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

    fn doc_with(seed: Vec<SeedFact>) -> IntentDoc {
        IntentDoc {
            seed,
            ..empty_doc()
        }
    }

    #[test]
    fn migrate_then_flatten_is_identity() {
        let doc = doc_with(vec![locus("oak_door"), locus("barrel")]);
        let bundle = migrate_doc(name("hearth"), name("main"), doc.clone()).unwrap();
        let flat = bundle.project.flatten(&bundle.modules).unwrap();
        assert_eq!(flat.doc, doc);
        assert_eq!(flat.anchors.len(), 2);
        assert!(
            flat.spans
                .iter()
                .any(|s| s.kind == SpanKind::Seed && s.index == 0)
        );
    }

    #[test]
    fn locus_and_mind_may_share_a_name() {
        let mut doc = doc_with(vec![locus("bran")]);
        doc.seed[0] = SeedFact::Locus {
            name: name("bran"),
            kind: LocusKind::Actor,
        };
        doc.minds = vec![MindSpec {
            locus: name("bran"),
            program: crate::MindProgram::default(),
            templates: Vec::new(),
        }];
        let bundle = migrate_doc(name("hearth"), name("main"), doc.clone()).unwrap();
        let flat = bundle.project.flatten(&bundle.modules).unwrap();
        assert_eq!(flat.doc, doc);
        assert_eq!(flat.anchors.len(), 2);
        assert_ne!(flat.anchors[0].anchor, flat.anchors[1].anchor);
    }

    #[test]
    fn module_reorder_does_not_change_flatten() {
        let a = migrate_doc(name("p"), name("alpha"), doc_with(vec![locus("a")])).unwrap();
        let b = migrate_doc(name("p"), name("beta"), doc_with(vec![locus("b")])).unwrap();
        let mut ma = a.modules.into_iter().next().unwrap();
        let mut mb = b.modules.into_iter().next().unwrap();
        ma.imports.clear();
        mb.imports.clear();
        let ha = ma.content_hash().unwrap();
        let hb = mb.content_hash().unwrap();
        let project = IntentProject {
            project: name("p"),
            modules: vec![
                IntentModuleRef {
                    id: name("beta"),
                    path: "modules/beta.ron".into(),
                    hash: hb,
                },
                IntentModuleRef {
                    id: name("alpha"),
                    path: "modules/alpha.ron".into(),
                    hash: ha,
                },
            ],
            lock: ModuleLock {
                entries: vec![
                    LockEntry {
                        id: name("beta"),
                        hash: hb,
                    },
                    LockEntry {
                        id: name("alpha"),
                        hash: ha,
                    },
                ],
            },
        };
        let swapped = project.flatten(&[mb.clone(), ma.clone()]).unwrap();
        let sorted_project = IntentProject {
            project: name("p"),
            modules: vec![
                IntentModuleRef {
                    id: name("alpha"),
                    path: "modules/alpha.ron".into(),
                    hash: ha,
                },
                IntentModuleRef {
                    id: name("beta"),
                    path: "modules/beta.ron".into(),
                    hash: hb,
                },
            ],
            lock: ModuleLock {
                entries: vec![
                    LockEntry {
                        id: name("alpha"),
                        hash: ha,
                    },
                    LockEntry {
                        id: name("beta"),
                        hash: hb,
                    },
                ],
            },
        };
        let canonical = sorted_project.flatten(&[ma, mb]).unwrap();
        assert_eq!(swapped.doc, canonical.doc);
        assert_eq!(swapped.doc.seed[0], locus("a"));
        assert_eq!(swapped.doc.seed[1], locus("b"));
    }

    #[test]
    fn import_cycle_fails_closed() {
        let a = migrate_doc(name("p"), name("a"), doc_with(vec![locus("x")])).unwrap();
        let b = migrate_doc(name("p"), name("b"), doc_with(vec![locus("y")])).unwrap();
        let mut ma = a.modules.into_iter().next().unwrap();
        let mut mb = b.modules.into_iter().next().unwrap();
        ma.imports = vec![ModuleImport {
            id: name("b"),
            hash: Hash::ZERO,
        }];
        mb.imports = vec![ModuleImport {
            id: name("a"),
            hash: Hash::ZERO,
        }];
        let ha = ma.content_hash().unwrap();
        let hb = mb.content_hash().unwrap();
        let project = IntentProject {
            project: name("p"),
            modules: vec![
                IntentModuleRef {
                    id: name("a"),
                    path: "modules/a.ron".into(),
                    hash: ha,
                },
                IntentModuleRef {
                    id: name("b"),
                    path: "modules/b.ron".into(),
                    hash: hb,
                },
            ],
            lock: ModuleLock {
                entries: vec![
                    LockEntry {
                        id: name("a"),
                        hash: ha,
                    },
                    LockEntry {
                        id: name("b"),
                        hash: hb,
                    },
                ],
            },
        };
        let err = project.flatten(&[ma, mb]).unwrap_err();
        match err {
            IrError::ImportCycle(ids) => {
                assert_eq!(ids, vec!["a".to_owned(), "b".to_owned()]);
            }
            other => panic!("{other}"),
        }
    }

    #[test]
    fn hash_drift_fails_closed() {
        let bundle = migrate_doc(name("p"), name("main"), doc_with(vec![locus("oak")])).unwrap();
        let mut project = bundle.project.clone();
        project.lock.entries[0].hash = Hash::from_bytes([1; 32]);
        project.modules[0].hash = Hash::from_bytes([1; 32]);
        let err = project.flatten(&bundle.modules).unwrap_err();
        match err {
            IrError::HashDrift { id, .. } => assert_eq!(id, "main"),
            other => panic!("{other}"),
        }
    }

    #[test]
    fn rename_keeps_anchor() {
        let doc = doc_with(vec![locus("oak_door")]);
        let mut bundle = migrate_doc(name("p"), name("main"), doc).unwrap();
        let module = &mut bundle.modules[0];
        let before = module.object_anchors[0].anchor;
        module.body.seed[0] = locus("front_door");
        module.object_anchors[0].name = name("front_door");
        module.exports = vec![name("front_door")];
        module.aliases = vec![NameAlias {
            name: name("oak_door"),
            target: before,
            until_epoch: 1,
        }];
        let hash = module.content_hash().unwrap();
        bundle.project.modules[0].hash = hash;
        bundle.project.lock.entries[0].hash = hash;
        let flat = bundle.project.flatten(&bundle.modules).unwrap();
        assert_eq!(flat.doc.seed[0], locus("front_door"));
        assert_eq!(flat.anchors[0].anchor, before);
        assert_eq!(flat.anchors[0].name.as_str(), "front_door");
    }

    #[test]
    fn tombstone_reuse_fails() {
        let mut bundle =
            migrate_doc(name("p"), name("main"), doc_with(vec![locus("oak")])).unwrap();
        let module = &mut bundle.modules[0];
        module.tombstones = vec![Tombstone {
            anchor: AnchorId::derive(b"gone", b"oak"),
            name: name("oak"),
            reason: "removed".into(),
        }];
        let err = module.validate_local().unwrap_err();
        assert!(matches!(err, IrError::TombstoneReuse(n) if n == "oak"));
    }

    #[test]
    fn unbound_parameter_fails_flatten() {
        let mut bundle = migrate_doc(name("p"), name("main"), empty_doc()).unwrap();
        let module = &mut bundle.modules[0];
        module.parameters = vec![ParameterDecl {
            name: name("scale"),
            ty: ParameterType::I32,
            default: None,
        }];
        let hash = module.content_hash().unwrap();
        bundle.project.modules[0].hash = hash;
        bundle.project.lock.entries[0].hash = hash;
        let err = bundle.project.flatten(&bundle.modules).unwrap_err();
        assert!(matches!(err, IrError::UnboundParameter(n) if n == "scale"));
    }

    #[test]
    fn defaulted_parameter_flattens() {
        let mut bundle = migrate_doc(name("p"), name("main"), empty_doc()).unwrap();
        let module = &mut bundle.modules[0];
        module.parameters = vec![ParameterDecl {
            name: name("scale"),
            ty: ParameterType::I32,
            default: Some(ParameterValue::I32(1)),
        }];
        let hash = module.content_hash().unwrap();
        bundle.project.modules[0].hash = hash;
        bundle.project.lock.entries[0].hash = hash;
        let flat = bundle.project.flatten(&bundle.modules).unwrap();
        assert_eq!(flat.doc, empty_doc());
    }

    #[test]
    fn project_ron_round_trip() {
        let bundle =
            migrate_doc(name("hearth"), name("main"), doc_with(vec![locus("chair")])).unwrap();
        let text = to_ron(&bundle.project).unwrap();
        let again: IntentProject = from_ron(&text).unwrap();
        assert_eq!(bundle.project, again);
        let module_text = to_ron(&bundle.modules[0]).unwrap();
        let module: IntentModule = from_ron(&module_text).unwrap();
        assert_eq!(bundle.modules[0], module);
    }

    #[test]
    fn unexpanded_pattern_fails_flatten() {
        let mut bundle =
            migrate_doc(name("p"), name("main"), doc_with(vec![locus("oak_door")])).unwrap();
        let instance = PatternInstance {
            anchor: bundle.modules[0].anchor.child(b"pattern:gate"),
            module: bundle.modules[0].anchor,
            instance: name("gate"),
            pattern: name("traversal.door_key"),
            version: 1,
            args: Vec::new(),
        };
        bundle.modules[0].object_anchors.push(ObjectAnchor {
            kind: AnchorKind::Pattern,
            name: name("gate"),
            anchor: instance.anchor,
        });
        bundle.modules[0].patterns.push(instance);
        let err = bundle.project.flatten(&bundle.modules).unwrap_err();
        match err {
            IrError::UnexpandedPattern(id) => assert_eq!(id, "traversal.door_key"),
            other => panic!("{other}"),
        }
    }

    #[test]
    fn missing_patterns_field_deserializes_empty() {
        let bundle =
            migrate_doc(name("hearth"), name("main"), doc_with(vec![locus("chair")])).unwrap();
        let mut text = to_ron(&bundle.modules[0]).unwrap();
        text = text.replace("patterns:[],", "");
        let module: IntentModule = from_ron(&text).unwrap();
        assert!(module.patterns.is_empty());
        assert_eq!(module.body, bundle.modules[0].body);
    }

    #[test]
    fn import_hash_mismatch_is_drift() {
        let a = migrate_doc(name("p"), name("a"), doc_with(vec![locus("x")])).unwrap();
        let b = migrate_doc(name("p"), name("b"), doc_with(vec![locus("y")])).unwrap();
        let mut ma = a.modules.into_iter().next().unwrap();
        let mb = b.modules.into_iter().next().unwrap();
        ma.imports = vec![ModuleImport {
            id: name("b"),
            hash: Hash::from_bytes([9; 32]),
        }];
        let ha = ma.content_hash().unwrap();
        let hb = mb.content_hash().unwrap();
        let project = IntentProject {
            project: name("p"),
            modules: vec![
                IntentModuleRef {
                    id: name("a"),
                    path: "modules/a.ron".into(),
                    hash: ha,
                },
                IntentModuleRef {
                    id: name("b"),
                    path: "modules/b.ron".into(),
                    hash: hb,
                },
            ],
            lock: ModuleLock {
                entries: vec![
                    LockEntry {
                        id: name("a"),
                        hash: ha,
                    },
                    LockEntry {
                        id: name("b"),
                        hash: hb,
                    },
                ],
            },
        };
        let err = project.flatten(&[ma, mb]).unwrap_err();
        assert!(matches!(err, IrError::HashDrift { id, .. } if id == "b"));
    }

    #[test]
    fn mind_and_style_concatenate_in_id_order() {
        let mut da = empty_doc();
        da.style.notes = "alpha".into();
        da.style.palettes = vec![name("stone")];
        da.minds = vec![MindSpec {
            locus: name("a_npc"),
            program: crate::MindProgram::default(),
            templates: Vec::new(),
        }];
        let mut db = empty_doc();
        db.style.notes = "beta".into();
        db.style.kitbash_tags = vec![name("prop.stool")];
        db.seed = vec![locus("prop")];
        let a = migrate_doc(name("p"), name("zulu"), da).unwrap();
        let b = migrate_doc(name("p"), name("able"), db).unwrap();
        let ma = a.modules.into_iter().next().unwrap();
        let mb = b.modules.into_iter().next().unwrap();
        let ha = ma.content_hash().unwrap();
        let hb = mb.content_hash().unwrap();
        let project = IntentProject {
            project: name("p"),
            modules: vec![
                IntentModuleRef {
                    id: name("zulu"),
                    path: "modules/zulu.ron".into(),
                    hash: ha,
                },
                IntentModuleRef {
                    id: name("able"),
                    path: "modules/able.ron".into(),
                    hash: hb,
                },
            ],
            lock: ModuleLock {
                entries: vec![
                    LockEntry {
                        id: name("zulu"),
                        hash: ha,
                    },
                    LockEntry {
                        id: name("able"),
                        hash: hb,
                    },
                ],
            },
        };
        let flat = project.flatten(&[ma, mb]).unwrap();
        assert_eq!(flat.doc.style.notes, "beta\nalpha");
        assert_eq!(flat.doc.style.palettes[0].as_str(), "stone");
        assert_eq!(flat.doc.minds[0].locus.as_str(), "a_npc");
        assert_eq!(flat.doc.seed[0], locus("prop"));
    }
}
