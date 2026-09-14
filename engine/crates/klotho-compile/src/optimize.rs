//! KAI-19 dual whole-title cook and equivalence gate.
//!
//! The reference cook remains the readable fallback. Optimizer passes are
//! applied in a fixed order and each semantic pass is retained only when a
//! trusted caller's finite replay corpus has identical events, rejects, and
//! terminal Trace prefixes. Presentation/layout passes produce an explicit
//! deterministic runtime plan and never rewrite semantic geometry.

use std::collections::{BTreeMap, BTreeSet};

use klotho_core::{AabbMm, BlobId, Hash, Sigil};
use klotho_ir::{
    AnchorId, AnchorKind, CanonDiff, Flattened, IntentDoc, IntentModule, IntentProject, Name, Pred,
    RiteGraph, RiteNode, RiteOp, SourceSpan, SpanKind, blame_anchor, to_ron,
};
use klotho_prove::Cas;
use klotho_trace::TraceDelta;

use crate::QualityTier;
use crate::cook::{Cooked, cook_doc};
use crate::error::{CompileError, check_ship_allowlist};
use crate::material::{ShaderPerm, prune_permutations};

/// Stable optimizer order. Reordering changes the optimized artifact hash.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
#[repr(u8)]
pub enum OptimizationPass {
    /// Fold algebraic predicate identities without inventing a Boolean atom.
    FoldPredicates = 1,
    /// Remove unreachable labeled Rite nodes after specialization.
    EliminateUnreachableRiteNodes = 2,
    /// Record that locked module parameters/patterns were materialized.
    SpecializePatterns = 3,
    /// Share byte-identical compiled predicate programs and Rite blobs.
    InternFragments = 4,
    /// Emit stable-id-to-storage table layouts.
    PackTables = 5,
    /// Build sorted Place bundles and access groups.
    PlanPlaces = 6,
    /// Keep only used material permutations for each SKU tier.
    PrunePermutations = 7,
    /// Exclude authoring/model/source-map inputs from the runtime plan.
    StripAuthoring = 8,
}

impl OptimizationPass {
    /// Stable human-readable pass id.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::FoldPredicates => "fold-predicates",
            Self::EliminateUnreachableRiteNodes => "eliminate-unreachable-rite-nodes",
            Self::SpecializePatterns => "specialize-patterns",
            Self::InternFragments => "intern-fragments",
            Self::PackTables => "pack-tables",
            Self::PlanPlaces => "plan-places",
            Self::PrunePermutations => "prune-permutations",
            Self::StripAuthoring => "strip-authoring",
        }
    }
}

/// Per-pass accounting and fallback state.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct PassReport {
    /// Pass.
    pub pass: OptimizationPass,
    /// Logical rows/bytes before the pass.
    pub before: usize,
    /// Logical rows/bytes after the pass.
    pub after: usize,
    /// Whether the candidate survived the equivalence gate.
    pub applied: bool,
    /// First divergent case when the pass fell back.
    pub fallback_case: Option<Name>,
}

/// Whole-title optimizer measurements. Ratios are diagnostics, not gates.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct OptimizationReport {
    /// Reports in [`OptimizationPass`] order.
    pub passes: Vec<PassReport>,
    /// Reference predicate rows.
    pub reference_predicates: usize,
    /// Optimized predicate rows.
    pub optimized_predicates: usize,
    /// Canonical reference document bytes.
    pub reference_doc_bytes: usize,
    /// Canonical optimized document bytes.
    pub optimized_doc_bytes: usize,
}

/// One deterministic replay/journey/fuzz outcome.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct TraceRun {
    /// Stable corpus case id.
    pub case: Name,
    /// Per-tick admitted events and legal rejects.
    pub deltas: Vec<TraceDelta>,
    /// Terminal committed Trace prefix.
    pub terminal_prefix: Hash,
}

/// Result of comparing a reference and optimized corpus.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct EquivalenceReport {
    /// Sorted case ids compared.
    pub cases: Vec<Name>,
}

/// Compare exact admitted/rejected behavior and terminal Trace prefixes.
pub fn compare_trace_runs(
    reference: &[TraceRun],
    optimized: &[TraceRun],
) -> Result<EquivalenceReport, CompileError> {
    if reference.is_empty() || optimized.is_empty() {
        return Err(CompileError::Equivalence("empty corpus".into()));
    }
    let index = |runs: &[TraceRun]| -> Result<BTreeMap<Name, usize>, CompileError> {
        let mut out = BTreeMap::new();
        for (position, run) in runs.iter().enumerate() {
            if out.insert(run.case.clone(), position).is_some() {
                return Err(CompileError::Equivalence(format!(
                    "duplicate case {}",
                    run.case.as_str()
                )));
            }
        }
        Ok(out)
    };
    let left = index(reference)?;
    let right = index(optimized)?;
    if left.keys().ne(right.keys()) {
        return Err(CompileError::Equivalence("corpus case set differs".into()));
    }
    for (case, a) in &left {
        let a = &reference[*a];
        let b = &optimized[right[case]];
        let same_deltas = a.deltas.len() == b.deltas.len()
            && a.deltas
                .iter()
                .zip(&b.deltas)
                .all(|(x, y)| x.tick == y.tick && x.events == y.events && x.rejects == y.rejects);
        if !same_deltas || a.terminal_prefix != b.terminal_prefix {
            return Err(CompileError::Equivalence(case.as_str().to_owned()));
        }
    }
    Ok(EquivalenceReport {
        cases: left.keys().cloned().collect(),
    })
}

/// One Place's whole-title asset and locality input.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct PlacePlanInput {
    /// Place identity.
    pub place: Sigil,
    /// Coarse residency bounds.
    pub aabb: AabbMm,
    /// Referenced CAS blobs. Duplicates are accepted and removed.
    pub assets: Vec<BlobId>,
    /// Measured deterministic access group.
    pub access_group: u16,
}

/// One SKU's used material permutations.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct SkuPlanInput {
    /// Stable SKU id.
    pub sku: Name,
    /// Quality floor.
    pub tier: QualityTier,
    /// Permutations reached by authored materials before tier pruning.
    pub used_permutations: Vec<ShaderPerm>,
}

/// Inputs that do not affect authoritative Canon semantics.
#[derive(Clone, Eq, PartialEq, Debug, Default)]
pub struct WholeTitleRequest {
    /// Place bundle inputs.
    pub places: Vec<PlacePlanInput>,
    /// SKU presentation inputs.
    pub skus: Vec<SkuPlanInput>,
    /// Candidate auxiliary files. Authoring-only paths are stripped.
    pub auxiliary_files: BTreeMap<String, Vec<u8>>,
}

/// Stable table identity mapped to a packed storage index.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct PackedRow {
    /// Semantic table name.
    pub table: &'static str,
    /// Stable semantic id.
    pub stable_id: u16,
    /// Physical index in the optimized table.
    pub storage_index: u16,
}

/// Sorted, deduplicated Place bundle.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct PlaceBundlePlan {
    /// Place identity.
    pub place: Sigil,
    /// Coarse residency bounds; presentation optimization cannot alter it.
    pub aabb: AabbMm,
    /// Sorted unique blob ids.
    pub assets: Vec<BlobId>,
    /// Deterministic locality group.
    pub access_group: u16,
}

/// Pruned presentation plan for one SKU.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct SkuBundlePlan {
    /// SKU id.
    pub sku: Name,
    /// Sorted packed permutation ids.
    pub permutations: Vec<u8>,
}

/// Explicit runtime layout selected by the optimized cook.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct WholeTitlePlan {
    /// Stable semantic table mappings.
    pub tables: Vec<PackedRow>,
    /// Place plans sorted by access group then Place identity.
    pub places: Vec<PlaceBundlePlan>,
    /// SKU plans sorted by id.
    pub skus: Vec<SkuBundlePlan>,
    /// Runtime-safe auxiliary files.
    pub runtime_files: BTreeMap<String, Vec<u8>>,
    /// Paths deliberately removed from the ship graph.
    pub stripped_paths: Vec<String>,
    /// Content hash of the deterministic encoded plan.
    pub hash: Hash,
}

/// Content-scale accounting over an optimized Place/CAS plan.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ScaleReport {
    /// Number of logical Places.
    pub places: usize,
    /// Total Place-to-blob references after per-Place deduplication.
    pub references: usize,
    /// Distinct referenced CAS blobs.
    pub unique_blobs: usize,
    /// Bytes occupied by distinct referenced blobs.
    pub unique_bytes: usize,
    /// Unique bytes plus the encoded runtime plan.
    pub package_bytes: usize,
    /// References per unique blob in fixed-point thousandths.
    pub instancing_ratio_milli: u32,
}

/// Hard product limits for a content-scale plan.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ScaleLimits {
    /// Maximum logical Places.
    pub places: usize,
    /// Maximum distinct referenced blobs.
    pub unique_blobs: usize,
    /// Maximum distinct blob bytes.
    pub unique_bytes: usize,
    /// Maximum complete package bytes.
    pub package_bytes: usize,
}

/// Measure unique/package bytes and enforce that every plan reference exists in CAS.
pub fn measure_scale(cas: &Cas, plan: &WholeTitlePlan) -> Result<ScaleReport, CompileError> {
    let references: usize = plan.places.iter().map(|place| place.assets.len()).sum();
    let unique: BTreeSet<_> = plan
        .places
        .iter()
        .flat_map(|place| place.assets.iter().copied())
        .collect();
    let mut unique_bytes = 0usize;
    for id in &unique {
        let bytes = cas
            .get(*id)
            .ok_or_else(|| CompileError::Optimization(format!("missing scale-report blob {id}")))?;
        unique_bytes = unique_bytes
            .checked_add(bytes.len())
            .ok_or_else(|| CompileError::Optimization("scale byte count overflow".into()))?;
    }
    let plan_bytes = encode_whole_title_plan(plan)?.len();
    let package_bytes = unique_bytes
        .checked_add(plan_bytes)
        .ok_or_else(|| CompileError::Optimization("package byte count overflow".into()))?;
    let instancing_ratio_milli = if unique.is_empty() {
        0
    } else {
        u32::try_from(references.saturating_mul(1_000) / unique.len()).unwrap_or(u32::MAX)
    };
    Ok(ScaleReport {
        places: plan.places.len(),
        references,
        unique_blobs: unique.len(),
        unique_bytes,
        package_bytes,
        instancing_ratio_milli,
    })
}

/// Fail when a measured scale plan exceeds any checked-in product cap.
pub fn check_scale(report: &ScaleReport, limits: &ScaleLimits) -> Result<(), CompileError> {
    if report.places > limits.places
        || report.unique_blobs > limits.unique_blobs
        || report.unique_bytes > limits.unique_bytes
        || report.package_bytes > limits.package_bytes
    {
        return Err(CompileError::Optimization(
            "content-scale plan exceeds product limits".into(),
        ));
    }
    Ok(())
}

/// Debug-only map from optimized semantic rows to authoring identity.
#[derive(Clone, Eq, PartialEq, Debug, Default)]
pub struct OptimizedSourceMap {
    /// Sorted source entries.
    pub entries: Vec<SourceMapEntry>,
}

/// One optimized row's source identity.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct SourceMapEntry {
    /// `law`, `affordance`, `rite`, or `beat`.
    pub table: &'static str,
    /// Stable table id.
    pub stable_id: u16,
    /// Immutable authoring identity.
    pub anchor: AnchorId,
    /// Original modular span when available.
    pub span: Option<SourceSpan>,
}

/// Reference artifact, proven optimized artifact, and diagnostics.
#[derive(Clone, Debug)]
pub struct WholeTitleCook {
    /// Simple readable cook.
    pub reference: Cooked,
    /// Candidate that passed the supplied corpus, with per-pass fallback.
    pub optimized: Cooked,
    /// Explicit deterministic layout/presentation plan.
    pub plan: WholeTitlePlan,
    /// Pass accounting.
    pub report: OptimizationReport,
    /// Trusted-corpus result for the final candidate.
    pub equivalence: EquivalenceReport,
    /// Debug-only author mapping. Not encoded into [`WholeTitlePlan`].
    pub source_map: OptimizedSourceMap,
}

/// Cook a flat title twice and gate every semantic optimization with `runner`.
pub fn cook_whole_title<F>(
    doc: &IntentDoc,
    request: &WholeTitleRequest,
    runner: F,
) -> Result<WholeTitleCook, CompileError>
where
    F: FnMut(&Cooked) -> Result<Vec<TraceRun>, CompileError>,
{
    cook_whole_title_inner(doc, request, None, 0, runner)
}

/// Flatten a locked project, retain its source identities, then dual-cook it.
pub fn cook_whole_project<F>(
    project: &IntentProject,
    modules: &[IntentModule],
    request: &WholeTitleRequest,
    runner: F,
) -> Result<WholeTitleCook, CompileError>
where
    F: FnMut(&Cooked) -> Result<Vec<TraceRun>, CompileError>,
{
    let flat = project
        .flatten(modules)
        .map_err(|e| CompileError::Flatten(e.to_string()))?;
    let specialized = modules.iter().map(|module| module.parameters.len()).sum();
    cook_whole_title_inner(&flat.doc, request, Some(&flat), specialized, runner)
}

fn cook_whole_title_inner<F>(
    doc: &IntentDoc,
    request: &WholeTitleRequest,
    flat: Option<&Flattened>,
    specialized: usize,
    mut runner: F,
) -> Result<WholeTitleCook, CompileError>
where
    F: FnMut(&Cooked) -> Result<Vec<TraceRun>, CompileError>,
{
    let reference = cook_doc(doc)?;
    let reference_runs = runner(&reference)?;
    compare_trace_runs(&reference_runs, &reference_runs)?;
    let reference_doc_bytes = doc_bytes(doc)?;
    let mut current = doc.clone();
    let mut reports = Vec::new();

    for pass in [
        OptimizationPass::FoldPredicates,
        OptimizationPass::EliminateUnreachableRiteNodes,
    ] {
        let before = doc_bytes(&current)?;
        let mut candidate = current.clone();
        let changed = match pass {
            OptimizationPass::FoldPredicates => fold_doc_predicates(&mut candidate),
            OptimizationPass::EliminateUnreachableRiteNodes => {
                eliminate_unreachable(&mut candidate)
            }
            _ => 0,
        };
        let after = doc_bytes(&candidate)?;
        let cooked = cook_doc(&candidate)?;
        let candidate_runs = runner(&cooked)?;
        match compare_trace_runs(&reference_runs, &candidate_runs) {
            Ok(_) => {
                current = candidate;
                reports.push(PassReport {
                    pass,
                    before,
                    after,
                    applied: true,
                    fallback_case: None,
                });
            }
            Err(CompileError::Equivalence(case)) => reports.push(PassReport {
                pass,
                before,
                after: before,
                applied: false,
                fallback_case: Some(Name::from(case.as_str())),
            }),
            Err(error) => return Err(error),
        }
        let _ = changed;
    }

    reports.push(PassReport {
        pass: OptimizationPass::SpecializePatterns,
        before: specialized,
        after: 0,
        applied: true,
        fallback_case: None,
    });

    let mut optimized = cook_doc(&current)?;
    let before_preds = optimized.canon.preds.len();
    let removed = optimized.canon.intern_predicates();
    optimized.optimized = true;
    optimized.cook_hash = crate::cook::optimized_cook_digest(optimized.cook_hash);
    let interned_runs = runner(&optimized)?;
    match compare_trace_runs(&reference_runs, &interned_runs) {
        Ok(_) => reports.push(PassReport {
            pass: OptimizationPass::InternFragments,
            before: before_preds,
            after: before_preds - removed,
            applied: true,
            fallback_case: None,
        }),
        Err(CompileError::Equivalence(case)) => {
            optimized = cook_doc(&current)?;
            reports.push(PassReport {
                pass: OptimizationPass::InternFragments,
                before: before_preds,
                after: before_preds,
                applied: false,
                fallback_case: Some(Name::from(case.as_str())),
            });
        }
        Err(error) => return Err(error),
    }

    let mut plan = build_plan(&optimized, request)?;
    reports.extend(plan_reports(&optimized, request, &plan));
    plan.hash = plan_hash(&plan)?;
    let final_runs = runner(&optimized)?;
    let equivalence = compare_trace_runs(&reference_runs, &final_runs)?;
    let source_map = build_source_map(&optimized, flat);

    Ok(WholeTitleCook {
        report: OptimizationReport {
            passes: reports,
            reference_predicates: reference.canon.preds.len(),
            optimized_predicates: optimized.canon.preds.len(),
            reference_doc_bytes,
            optimized_doc_bytes: doc_bytes(&optimized.doc)?,
        },
        reference,
        optimized,
        plan,
        equivalence,
        source_map,
    })
}

fn fold_doc_predicates(doc: &mut IntentDoc) -> usize {
    let mut changed = 0;
    for diff in &mut doc.canon_diffs {
        match diff {
            CanonDiff::AddLaw(law) => {
                changed += fold_pred(&mut law.when);
                match &mut law.body {
                    klotho_ir::LawBody::Pred { must, .. } => changed += fold_pred(must),
                    klotho_ir::LawBody::Cap { mark, .. } => changed += fold_pred(mark),
                    _ => {}
                }
            }
            CanonDiff::AddAffordance(affordance) => {
                for pred in &mut affordance.requires {
                    changed += fold_pred(pred);
                }
            }
            CanonDiff::AddRite(rite) => {
                for node in &mut rite.nodes {
                    let op = match node {
                        RiteNode::Op(op) | RiteNode::Labeled { op, .. } => op,
                    };
                    match op {
                        RiteOp::Guard(pred, _) | RiteOp::Branch(pred, _, _) => {
                            changed += fold_pred(pred);
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    changed
}

fn fold_pred(pred: &mut Pred) -> usize {
    let mut changed = 0;
    match pred {
        Pred::And(a, b) | Pred::Or(a, b) => {
            changed += fold_pred(a);
            changed += fold_pred(b);
            if a == b {
                *pred = (**a).clone();
                changed += 1;
            }
        }
        Pred::Not(inner) => {
            changed += fold_pred(inner);
            if let Pred::Not(value) = inner.as_ref() {
                *pred = (**value).clone();
                changed += 1;
            }
        }
        Pred::ExistsRelated { pred, .. } | Pred::CountRelated { pred, .. } => {
            changed += fold_pred(pred);
        }
        _ => {}
    }
    changed
}

fn eliminate_unreachable(doc: &mut IntentDoc) -> usize {
    let mut removed = 0;
    for diff in &mut doc.canon_diffs {
        let CanonDiff::AddRite(rite) = diff else {
            continue;
        };
        removed += eliminate_unreachable_labeled(rite);
    }
    removed
}

fn eliminate_unreachable_labeled(rite: &mut RiteGraph) -> usize {
    if !rite
        .nodes
        .iter()
        .all(|node| matches!(node, RiteNode::Labeled { .. }))
    {
        return 0;
    }
    let order: Vec<u16> = rite
        .nodes
        .iter()
        .filter_map(|node| match node {
            RiteNode::Labeled { pc, .. } => Some(*pc),
            RiteNode::Op(_) => None,
        })
        .collect();
    let ops: BTreeMap<u16, &RiteOp> = rite
        .nodes
        .iter()
        .filter_map(|node| match node {
            RiteNode::Labeled { pc, op } => Some((*pc, op)),
            RiteNode::Op(_) => None,
        })
        .collect();
    let mut reachable = BTreeSet::new();
    let mut stack = vec![rite.entry];
    while let Some(pc) = stack.pop() {
        if !reachable.insert(pc) {
            continue;
        }
        let Some(op) = ops.get(&pc) else { continue };
        let next = order
            .iter()
            .position(|known| *known == pc)
            .and_then(|i| order.get(i + 1))
            .copied();
        match op {
            RiteOp::Halt(_) | RiteOp::Complete(_) => {}
            RiteOp::Guard(_, fail) | RiteOp::Spend(_, _, fail) => {
                stack.push(*fail);
                if let Some(next) = next {
                    stack.push(next);
                }
            }
            RiteOp::Branch(_, yes, no) => {
                stack.push(*yes);
                stack.push(*no);
            }
            _ => {
                if let Some(next) = next {
                    stack.push(next);
                }
            }
        }
    }
    let before = rite.nodes.len();
    rite.nodes.retain(|node| match node {
        RiteNode::Labeled { pc, .. } => reachable.contains(pc),
        RiteNode::Op(_) => true,
    });
    before - rite.nodes.len()
}

fn build_plan(
    cooked: &Cooked,
    request: &WholeTitleRequest,
) -> Result<WholeTitlePlan, CompileError> {
    let mut places = Vec::new();
    let mut seen_places = BTreeSet::new();
    for input in &request.places {
        if !seen_places.insert(input.place) {
            return Err(CompileError::Optimization("duplicate Place".into()));
        }
        let mut assets = input.assets.clone();
        assets.sort();
        assets.dedup();
        if let Some(missing) = assets.iter().find(|id| !cooked.cas.contains(**id)) {
            return Err(CompileError::Optimization(format!(
                "missing Place blob {missing}"
            )));
        }
        places.push(PlaceBundlePlan {
            place: input.place,
            aabb: input.aabb,
            assets,
            access_group: input.access_group,
        });
    }
    places.sort_by_key(|place| (place.access_group, place.place));

    let mut skus = Vec::new();
    let mut seen_skus = BTreeSet::new();
    for input in &request.skus {
        if !seen_skus.insert(input.sku.clone()) {
            return Err(CompileError::Optimization(format!(
                "duplicate SKU {}",
                input.sku.as_str()
            )));
        }
        let permutations = prune_permutations(&input.used_permutations, input.tier)
            .into_iter()
            .map(ShaderPerm::bits)
            .collect();
        skus.push(SkuBundlePlan {
            sku: input.sku.clone(),
            permutations,
        });
    }
    skus.sort_by(|a, b| a.sku.cmp(&b.sku));

    let mut runtime_files = BTreeMap::new();
    let mut stripped_paths = Vec::new();
    for (path, bytes) in &request.auxiliary_files {
        match check_ship_allowlist(path) {
            Ok(()) if is_runtime_aux(path) => {
                runtime_files.insert(path.clone(), bytes.clone());
            }
            _ => stripped_paths.push(path.clone()),
        }
    }
    stripped_paths.sort();

    Ok(WholeTitlePlan {
        tables: pack_tables(cooked),
        places,
        skus,
        runtime_files,
        stripped_paths,
        hash: Hash::ZERO,
    })
}

fn is_runtime_aux(path: &str) -> bool {
    let path = path.replace('\\', "/");
    [
        "runtime/", "places/", "cas/", "shaders/", "locales/", "audio/",
    ]
    .iter()
    .any(|prefix| path.starts_with(prefix))
}

fn pack_tables(cooked: &Cooked) -> Vec<PackedRow> {
    let mut rows = Vec::new();
    let mut push = |table, len: usize| {
        for stable_id in 0..len {
            rows.push(PackedRow {
                table,
                stable_id: stable_id as u16,
                storage_index: stable_id as u16,
            });
        }
    };
    push("law", cooked.canon.laws.len());
    push("affordance", cooked.canon.affordances.len());
    push("rite", cooked.canon.rites.len());
    push("predicate", cooked.canon.preds.len());
    rows
}

fn plan_reports(
    cooked: &Cooked,
    request: &WholeTitleRequest,
    plan: &WholeTitlePlan,
) -> Vec<PassReport> {
    let used_perms: usize = request
        .skus
        .iter()
        .map(|sku| sku.used_permutations.len())
        .sum();
    let kept_perms: usize = plan.skus.iter().map(|sku| sku.permutations.len()).sum();
    let input_assets: usize = request.places.iter().map(|place| place.assets.len()).sum();
    let packed_assets: usize = plan.places.iter().map(|place| place.assets.len()).sum();
    let input_bytes: usize = request.auxiliary_files.values().map(Vec::len).sum();
    let runtime_bytes: usize = plan.runtime_files.values().map(Vec::len).sum();
    vec![
        PassReport {
            pass: OptimizationPass::PackTables,
            before: cooked.canon.laws.len()
                + cooked.canon.affordances.len()
                + cooked.canon.rites.len()
                + cooked.canon.preds.len(),
            after: plan.tables.len(),
            applied: true,
            fallback_case: None,
        },
        PassReport {
            pass: OptimizationPass::PlanPlaces,
            before: input_assets,
            after: packed_assets,
            applied: true,
            fallback_case: None,
        },
        PassReport {
            pass: OptimizationPass::PrunePermutations,
            before: used_perms,
            after: kept_perms,
            applied: true,
            fallback_case: None,
        },
        PassReport {
            pass: OptimizationPass::StripAuthoring,
            before: input_bytes,
            after: runtime_bytes,
            applied: true,
            fallback_case: None,
        },
    ]
}

fn build_source_map(cooked: &Cooked, flat: Option<&Flattened>) -> OptimizedSourceMap {
    let mut anchors = BTreeMap::new();
    let mut diff_spans = BTreeMap::new();
    if let Some(flat) = flat {
        for object in &flat.anchors {
            anchors.insert((object.kind, object.name.clone()), object.anchor);
        }
        for span in &flat.spans {
            if span.kind == SpanKind::CanonDiff {
                diff_spans.insert(span.index, span.clone());
            }
        }
    }
    let mut named_spans = BTreeMap::new();
    for (index, diff) in cooked.doc.canon_diffs.iter().enumerate() {
        let named = match diff {
            CanonDiff::AddLaw(value) => Some((AnchorKind::Law, &value.id)),
            CanonDiff::AddAffordance(value) => Some((AnchorKind::Affordance, &value.id)),
            CanonDiff::AddRite(value) => Some((AnchorKind::Rite, &value.id)),
            CanonDiff::AddBeat(value) => Some((AnchorKind::Beat, &value.id)),
            CanonDiff::RetractLaw { .. } | CanonDiff::RetractRite { .. } => None,
        };
        if let (Some(key), Some(span)) = (named, diff_spans.get(&(index as u32))) {
            named_spans.insert((key.0, key.1.clone()), span.clone());
        }
    }
    let mut entries = Vec::new();
    let mut add = |table: &'static str, kind: AnchorKind, stable_id: usize, name: &Name| {
        entries.push(SourceMapEntry {
            table,
            stable_id: stable_id as u16,
            anchor: anchors
                .get(&(kind, name.clone()))
                .copied()
                .unwrap_or_else(|| blame_anchor(table, name.as_str())),
            span: named_spans.get(&(kind, name.clone())).cloned(),
        });
    };
    for (i, law) in cooked.canon.laws.iter().enumerate() {
        add("law", AnchorKind::Law, i, &law.name);
    }
    for (i, aff) in cooked.canon.affordances.iter().enumerate() {
        add("affordance", AnchorKind::Affordance, i, &aff.name);
    }
    for (i, rite) in cooked.canon.rites.iter().enumerate() {
        add("rite", AnchorKind::Rite, i, &rite.name);
    }
    for (i, beat) in cooked.canon.beats.iter().enumerate() {
        add("beat", AnchorKind::Beat, i, &beat.id);
    }
    entries.sort_by_key(|entry| (entry.table, entry.stable_id));
    OptimizedSourceMap { entries }
}

fn doc_bytes(doc: &IntentDoc) -> Result<usize, CompileError> {
    to_ron(doc)
        .map(|text| text.len())
        .map_err(|error| CompileError::Optimization(error.to_string()))
}

/// Deterministic runtime plan bytes. Source maps and fallback diagnostics are
/// deliberately absent.
pub fn encode_whole_title_plan(plan: &WholeTitlePlan) -> Result<Vec<u8>, CompileError> {
    let mut out = Vec::new();
    out.extend_from_slice(b"KOPT");
    out.push(1);
    put_len(&mut out, plan.tables.len())?;
    for row in &plan.tables {
        put_str(&mut out, row.table)?;
        out.extend_from_slice(&row.stable_id.to_le_bytes());
        out.extend_from_slice(&row.storage_index.to_le_bytes());
    }
    put_len(&mut out, plan.places.len())?;
    for place in &plan.places {
        out.extend_from_slice(&place.place.raw().to_le_bytes());
        for value in [
            place.aabb.min.x,
            place.aabb.min.y,
            place.aabb.min.z,
            place.aabb.max.x,
            place.aabb.max.y,
            place.aabb.max.z,
        ] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&place.access_group.to_le_bytes());
        put_len(&mut out, place.assets.len())?;
        for asset in &place.assets {
            out.extend_from_slice(asset.as_bytes());
        }
    }
    put_len(&mut out, plan.skus.len())?;
    for sku in &plan.skus {
        put_str(&mut out, sku.sku.as_str())?;
        put_len(&mut out, sku.permutations.len())?;
        out.extend_from_slice(&sku.permutations);
    }
    put_len(&mut out, plan.runtime_files.len())?;
    for (path, bytes) in &plan.runtime_files {
        put_str(&mut out, path)?;
        put_len(&mut out, bytes.len())?;
        out.extend_from_slice(bytes);
    }
    Ok(out)
}

/// Decode and validate a model-free whole-title runtime plan.
pub fn decode_whole_title_plan(bytes: &[u8]) -> Result<WholeTitlePlan, CompileError> {
    const MAX_ROWS: usize = u16::MAX as usize;
    const MAX_FILES: usize = 16_384;
    let mut rest = bytes;
    if take(&mut rest, 4)? != b"KOPT" || take_u8(&mut rest)? != 1 {
        return Err(CompileError::Optimization("plan magic/version".into()));
    }
    let mut tables = Vec::new();
    for _ in 0..take_count(&mut rest, MAX_ROWS)? {
        let table = match take_str(&mut rest)? {
            "law" => "law",
            "affordance" => "affordance",
            "rite" => "rite",
            "predicate" => "predicate",
            _ => return Err(CompileError::Optimization("unknown table".into())),
        };
        tables.push(PackedRow {
            table,
            stable_id: take_u16(&mut rest)?,
            storage_index: take_u16(&mut rest)?,
        });
    }
    let mut places = Vec::new();
    for _ in 0..take_count(&mut rest, MAX_ROWS)? {
        let place = Sigil::from_raw(take_u128(&mut rest)?);
        if place.kind() != Some(klotho_core::LocusKind::Place) {
            return Err(CompileError::Optimization("non-Place bundle".into()));
        }
        let mut values = [0; 6];
        for value in &mut values {
            *value = take_i32(&mut rest)?;
        }
        let access_group = take_u16(&mut rest)?;
        let mut assets = Vec::new();
        for _ in 0..take_count(&mut rest, MAX_FILES)? {
            assets.push(BlobId::from_bytes(
                take(&mut rest, 32)?.try_into().expect("32"),
            ));
        }
        if !assets.windows(2).all(|pair| pair[0] < pair[1]) {
            return Err(CompileError::Optimization("unsorted Place assets".into()));
        }
        places.push(PlaceBundlePlan {
            place,
            aabb: AabbMm::new(
                klotho_core::IVec3 {
                    x: values[0],
                    y: values[1],
                    z: values[2],
                },
                klotho_core::IVec3 {
                    x: values[3],
                    y: values[4],
                    z: values[5],
                },
            ),
            assets,
            access_group,
        });
    }
    if !places
        .windows(2)
        .all(|pair| (pair[0].access_group, pair[0].place) < (pair[1].access_group, pair[1].place))
    {
        return Err(CompileError::Optimization("unsorted Places".into()));
    }
    let mut skus = Vec::new();
    for _ in 0..take_count(&mut rest, MAX_FILES)? {
        let sku = Name::from(take_str(&mut rest)?);
        let count = take_count(&mut rest, 16)?;
        let permutations = take(&mut rest, count)?.to_vec();
        if !permutations.windows(2).all(|pair| pair[0] < pair[1]) {
            return Err(CompileError::Optimization("unsorted permutations".into()));
        }
        skus.push(SkuBundlePlan { sku, permutations });
    }
    if !skus.windows(2).all(|pair| pair[0].sku < pair[1].sku) {
        return Err(CompileError::Optimization("unsorted SKUs".into()));
    }
    let mut runtime_files = BTreeMap::new();
    for _ in 0..take_count(&mut rest, MAX_FILES)? {
        let path = take_str(&mut rest)?.to_owned();
        if !is_runtime_aux(&path) || check_ship_allowlist(&path).is_err() {
            return Err(CompileError::Optimization(format!(
                "unsafe runtime path {path}"
            )));
        }
        let len = take_count(&mut rest, crate::WARP_CAP_DESKTOP)?;
        let contents = take(&mut rest, len)?.to_vec();
        if runtime_files.insert(path, contents).is_some() {
            return Err(CompileError::Optimization("duplicate runtime path".into()));
        }
    }
    if !rest.is_empty() {
        return Err(CompileError::Optimization("trailing plan bytes".into()));
    }
    Ok(WholeTitlePlan {
        tables,
        places,
        skus,
        runtime_files,
        stripped_paths: Vec::new(),
        hash: crate::digest_of(bytes),
    })
}

fn plan_hash(plan: &WholeTitlePlan) -> Result<Hash, CompileError> {
    Ok(crate::digest_of(&encode_whole_title_plan(plan)?))
}

fn put_len(out: &mut Vec<u8>, len: usize) -> Result<(), CompileError> {
    let len = u32::try_from(len).map_err(|_| CompileError::Optimization("plan length".into()))?;
    out.extend_from_slice(&len.to_le_bytes());
    Ok(())
}

fn put_str(out: &mut Vec<u8>, value: &str) -> Result<(), CompileError> {
    put_len(out, value.len())?;
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

fn take<'a>(rest: &mut &'a [u8], len: usize) -> Result<&'a [u8], CompileError> {
    if rest.len() < len {
        return Err(CompileError::Optimization("truncated plan".into()));
    }
    let (head, tail) = rest.split_at(len);
    *rest = tail;
    Ok(head)
}

fn take_u8(rest: &mut &[u8]) -> Result<u8, CompileError> {
    Ok(take(rest, 1)?[0])
}

fn take_u16(rest: &mut &[u8]) -> Result<u16, CompileError> {
    Ok(u16::from_le_bytes(take(rest, 2)?.try_into().expect("2")))
}

fn take_u32(rest: &mut &[u8]) -> Result<u32, CompileError> {
    Ok(u32::from_le_bytes(take(rest, 4)?.try_into().expect("4")))
}

fn take_i32(rest: &mut &[u8]) -> Result<i32, CompileError> {
    Ok(i32::from_le_bytes(take(rest, 4)?.try_into().expect("4")))
}

fn take_u128(rest: &mut &[u8]) -> Result<u128, CompileError> {
    Ok(u128::from_le_bytes(take(rest, 16)?.try_into().expect("16")))
}

fn take_count(rest: &mut &[u8], cap: usize) -> Result<usize, CompileError> {
    let count = take_u32(rest)? as usize;
    if count > cap {
        return Err(CompileError::Optimization("plan count cap".into()));
    }
    Ok(count)
}

fn take_str<'a>(rest: &mut &'a [u8]) -> Result<&'a str, CompileError> {
    let len = take_count(rest, 64 * 1024)?;
    std::str::from_utf8(take(rest, len)?)
        .map_err(|_| CompileError::Optimization("plan utf8".into()))
}

#[cfg(test)]
mod tests {
    use klotho_core::{LocusKind, Tick};
    use klotho_ir::{Law, LawBody, ProvenanceId, SeedFact, SourceKind, StyleIntent};
    use klotho_trace::TraceDelta;

    use super::*;

    fn doc() -> IntentDoc {
        let atom = Pred::SourceIs(SourceKind::Player);
        IntentDoc {
            style: StyleIntent::default(),
            canon_diffs: vec![CanonDiff::AddLaw(Law {
                id: Name::from("same"),
                when: Pred::And(Box::new(atom.clone()), Box::new(atom.clone())),
                body: LawBody::Pred {
                    must: Pred::Not(Box::new(Pred::Not(Box::new(atom)))),
                    ought: None,
                },
            })],
            seed: vec![SeedFact::Locus {
                name: Name::from("player"),
                kind: LocusKind::Actor,
            }],
            minds: Vec::new(),
            provenance: ProvenanceId(Hash::ZERO),
        }
    }

    fn stable(_: &Cooked) -> Result<Vec<TraceRun>, CompileError> {
        Ok(vec![TraceRun {
            case: Name::from("finite"),
            deltas: vec![TraceDelta::empty(Tick(1))],
            terminal_prefix: Hash::ZERO,
        }])
    }

    #[test]
    fn dual_cook_is_deterministic_and_folds_safe_identities() {
        let mut request = WholeTitleRequest::default();
        request
            .auxiliary_files
            .insert("models/critic.gguf".into(), vec![1]);
        request
            .auxiliary_files
            .insert("runtime/nav.bin".into(), vec![2]);
        let a = cook_whole_title(&doc(), &request, stable).unwrap();
        let b = cook_whole_title(&doc(), &request, stable).unwrap();
        assert!(a.optimized.optimized);
        assert!(a.report.optimized_doc_bytes < a.report.reference_doc_bytes);
        assert_eq!(a.plan.hash, b.plan.hash);
        assert_eq!(a.plan.stripped_paths, vec!["models/critic.gguf"]);
        assert!(a.plan.runtime_files.contains_key("runtime/nav.bin"));
        assert_eq!(
            a.report
                .passes
                .iter()
                .map(|p| p.pass as u8)
                .collect::<Vec<_>>(),
            (1..=8).collect::<Vec<_>>()
        );
    }

    #[test]
    fn divergent_pass_falls_back_to_reference_shape() {
        let reference_len = doc_bytes(&doc()).unwrap();
        let cook = cook_whole_title(&doc(), &WholeTitleRequest::default(), |cooked| {
            let prefix = if doc_bytes(&cooked.doc).unwrap() == reference_len {
                Hash::ZERO
            } else {
                Hash::from_bytes([9; 32])
            };
            Ok(vec![TraceRun {
                case: Name::from("budget-edge"),
                deltas: vec![TraceDelta::empty(Tick(1))],
                terminal_prefix: prefix,
            }])
        })
        .unwrap();
        let fold = &cook.report.passes[0];
        assert!(!fold.applied);
        assert_eq!(
            fold.fallback_case.as_ref().map(Name::as_str),
            Some("budget-edge")
        );
        assert_eq!(
            cook.report.reference_doc_bytes,
            cook.report.optimized_doc_bytes
        );
    }

    #[test]
    fn exact_rejects_are_part_of_equivalence() {
        let a = TraceRun {
            case: Name::from("x"),
            deltas: vec![TraceDelta::empty(Tick(1))],
            terminal_prefix: Hash::ZERO,
        };
        let mut b = a.clone();
        b.deltas[0].rejects.push((
            klotho_trace::ProposalKind::Player,
            klotho_core::RejectReason::Budget,
        ));
        assert!(compare_trace_runs(&[a], &[b]).is_err());
    }

    #[test]
    fn unreachable_labeled_rite_nodes_are_removed_in_author_order() {
        let mut rite = RiteGraph {
            id: Name::from("specialized"),
            cap_steps: 8,
            cap_ticks: 8,
            entry: 10,
            nodes: vec![
                RiteNode::Labeled {
                    pc: 10,
                    op: RiteOp::Complete(klotho_ir::Status::Success),
                },
                RiteNode::Labeled {
                    pc: 2,
                    op: RiteOp::Complete(klotho_ir::Status::Fail),
                },
            ],
        };
        assert_eq!(eliminate_unreachable_labeled(&mut rite), 1);
        assert!(matches!(rite.nodes[0], RiteNode::Labeled { pc: 10, .. }));
    }

    #[test]
    fn locked_project_source_map_keeps_immutable_anchor_and_span() {
        let bundle = klotho_ir::migrate_doc(Name::from("p"), Name::from("main"), doc()).unwrap();
        let expected = bundle.modules[0]
            .object_anchors
            .iter()
            .find(|object| object.kind == AnchorKind::Law)
            .unwrap()
            .anchor;
        let cooked = cook_whole_project(
            &bundle.project,
            &bundle.modules,
            &WholeTitleRequest::default(),
            stable,
        )
        .unwrap();
        let law = cooked
            .source_map
            .entries
            .iter()
            .find(|entry| entry.table == "law")
            .unwrap();
        assert_eq!(law.anchor, expected);
        assert!(law.span.is_some());
    }

    #[test]
    fn place_and_sku_plans_sort_dedup_and_prune() {
        let cooked = cook_doc(&doc()).unwrap();
        let mut request = WholeTitleRequest::default();
        let place = Sigil::pack(LocusKind::Place, 0, 2).unwrap();
        request.places.push(PlacePlanInput {
            place,
            aabb: AabbMm::from_point(klotho_core::IVec3::ZERO),
            assets: Vec::new(),
            access_group: 3,
        });
        request.skus.push(SkuPlanInput {
            sku: Name::from("low"),
            tier: QualityTier::Low,
            used_permutations: vec![ShaderPerm {
                albedo_tex: true,
                metal_rough: true,
                emissive: true,
                alpha_test: false,
            }],
        });
        let plan = build_plan(&cooked, &request).unwrap();
        assert_eq!(plan.places[0].place, place);
        assert_eq!(plan.skus[0].permutations, vec![1]);
        assert!(
            encode_whole_title_plan(&plan)
                .unwrap()
                .starts_with(b"KOPT\x01")
        );
        let bytes = encode_whole_title_plan(&plan).unwrap();
        let decoded = decode_whole_title_plan(&bytes).unwrap();
        assert_eq!(decoded.tables, plan.tables);
        assert_eq!(decoded.places, plan.places);
        assert_eq!(decoded.skus, plan.skus);
        assert_eq!(decoded.hash, crate::digest_of(&bytes));
        let mut trailing = bytes;
        trailing.push(0);
        assert!(decode_whole_title_plan(&trailing).is_err());
    }
}
