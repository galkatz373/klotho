#![forbid(unsafe_code)]
//! KAI-00 benchmark contract validation and evidence report generation.

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const PUBLIC_FILES: [&str; 3] = [
    "benchmarks/kai/v1/public/mechanic.ron",
    "benchmarks/kai/v1/public/content.ron",
    "benchmarks/kai/v1/assets/public.ron",
];
const METRICS: [&str; 11] = [
    "engine_native_workflow",
    "first_playable_latency",
    "common_mechanic_latency",
    "content_iteration_latency",
    "art_cold_start_latency",
    "pattern_capability_coverage",
    "title_code_escape_rate",
    "engineer_burden",
    "repair_autonomy",
    "regression_containment",
    "reproducibility",
];

/// One public benchmark corpus partition.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Corpus {
    /// Schema major.
    pub version: u16,
    /// Reusable acceptance contracts.
    pub acceptance_profiles: Vec<AcceptanceProfile>,
    /// Immutable request outcomes.
    pub outcomes: Vec<Outcome>,
}

/// Contract applied to an outcome.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceProfile {
    /// Stable profile name.
    pub id: String,
    /// Semantic hard gates.
    pub invariants: Vec<String>,
    /// Required playable evidence.
    pub journeys: Vec<String>,
    /// Reference and taste constraints.
    pub quality_references: Vec<String>,
    /// Accessibility requirements.
    pub accessibility: Vec<String>,
    /// Named platform budgets.
    pub budgets: Vec<String>,
}

/// One request, permanently identified by one OutcomeId.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    /// Immutable outcome id; retries and operations retain this id.
    pub id: String,
    /// mechanic, content, or asset.
    pub domain: String,
    /// simple, composed, or novel.
    pub difficulty: String,
    /// Fixed benchmark request.
    pub input: String,
    /// Acceptance profile id.
    pub acceptance: String,
    /// Maximum permitted transaction scope.
    pub maximum_scope: Scope,
    /// Only questions the harness may answer.
    pub clarifications: Vec<Clarification>,
}

/// A bounded authoring transaction scope.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    /// Semantic operation cap.
    pub semantic_operations: u16,
    /// Source file cap.
    pub files: u16,
    /// New or modified asset cap.
    pub assets: u16,
}

/// A pre-authorized benchmark clarification and fixed response.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Clarification {
    /// Stable question id.
    pub id: String,
    /// Exact allowed question.
    pub question: String,
    /// Exact harness response.
    pub answer: String,
    /// Simulated response latency, charged to wall clock.
    pub latency_ms: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DifficultyCatalog {
    version: u16,
    entries: Vec<DifficultyEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DifficultyEntry {
    id: String,
    domain: String,
    difficulty: String,
    visibility: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HeldOutCatalog {
    version: u16,
    algorithm: String,
    escrow: EscrowReceipt,
    entries: Vec<HeldOutEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EscrowReceipt {
    owner: String,
    custodian_role: String,
    receipt_id: String,
    location_class: String,
    rotation_policy: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HeldOutEntry {
    id: String,
    domain: String,
    difficulty: String,
    prompt_sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TimingContract {
    version: u16,
    primary_clock: String,
    start_event: String,
    stop_event: String,
    never_paused_for: Vec<String>,
    separately_reported: Vec<String>,
    public_trials: u16,
    held_out_trials: u16,
    order: String,
    cold_cache: CacheContract,
    warm_cache: CacheContract,
    required_metrics: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CacheContract {
    id: String,
    derived_cache: String,
    source_assets: String,
    manifest_hash: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MachineManifest {
    version: u16,
    id: String,
    owner: String,
    manufacturer: String,
    model: String,
    cpu: String,
    cpu_cores: String,
    microcode_or_firmware: String,
    ram: String,
    storage: String,
    filesystem: String,
    gpu: String,
    gpu_firmware: String,
    driver: String,
    api: String,
    os_image: String,
    kernel: String,
    compiler: String,
    display: String,
    controller: String,
    polling_hz: u32,
    power_governor: String,
    thermal_precondition: String,
    process_affinity: String,
    claim_level: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FarmManifest {
    version: u16,
    id: String,
    owner: String,
    worker_classes: Vec<WorkerClass>,
    max_concurrency: u16,
    queue_policy: String,
    cas_topology: String,
    cache_topology: String,
    storage_bandwidth: String,
    network_bandwidth: String,
    network_latency: String,
    retry_policy: String,
    scheduling: String,
    measured_jobs_per_week: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerClass {
    machine_id: String,
    count: u16,
    slots_each: u16,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelLock {
    version: u16,
    lane: String,
    backend: String,
    model_id: String,
    artifact_path: String,
    model_artifact_sha256: String,
    sampling: String,
    context_tokens: u32,
    tool_catalog: String,
    network_policy: String,
    claim_level: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FaultCatalog {
    version: u16,
    entries: Vec<FaultEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FaultEntry {
    id: String,
    class: String,
    seed: String,
    expected_code: String,
    legal_repair: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SkuManifest {
    version: u16,
    id: String,
    os: String,
    graphics_api: String,
    resolution: String,
    quality: String,
    refresh_hz: u16,
    vrr: bool,
    controller: String,
    package_claim_level: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProgramPlan {
    version: u16,
    program_owner: String,
    currency_overlay: String,
    entries: Vec<PlanEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanEntry {
    id: String,
    wave: String,
    funding: String,
    accountable_owner: String,
    critical_path_weeks: u16,
    contingency_percent: u16,
    roles: Vec<RoleCapacity>,
    reviewer_hours_per_week: u16,
    costs: CostEnvelope,
    queues: Vec<QueueCapacity>,
    confidence: String,
    estimate_basis: String,
    actuals: String,
    reforecast_trigger: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoleCapacity {
    role: String,
    owner: String,
    estimated_fte_weeks: u16,
    available_fte_weeks: u16,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CostEnvelope {
    model_tokens_million: u32,
    cpu_hours: u32,
    gpu_hours: u32,
    storage_gib: u32,
    egress_gib: u32,
    dcc_seats: u16,
    vendor_weeks: u16,
    device_lab_weeks: u16,
    devkits: u16,
    localization_words: u32,
    support_weeks: u16,
    redacted_range: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueueCapacity {
    queue: String,
    arrivals_per_week: u32,
    service_per_week: u32,
    maximum_backlog: u32,
}

/// Successful repository validation totals.
#[derive(Clone, Debug, Serialize)]
pub struct ValidationSummary {
    /// Public request count.
    pub public_outcomes: usize,
    /// Held-out request count.
    pub held_out_outcomes: usize,
    /// Total immutable outcomes.
    pub total_outcomes: usize,
    /// Planned merge units.
    pub planned_prs: usize,
}

/// One emitted metric, including explicit not-yet-run state.
#[derive(Debug, Serialize)]
pub struct MetricReport {
    metric: String,
    status: String,
    sample_count: u64,
    p50: Option<u64>,
    p95: Option<u64>,
    unit: String,
}

/// Environment-bound validation report.
#[derive(Debug, Serialize)]
pub struct BenchmarkReport {
    schema: String,
    generated_unix_ms: u128,
    corpus_version: u16,
    environment_hashes: BTreeMap<String, String>,
    validation: ValidationSummary,
    metrics: Vec<MetricReport>,
}

fn load<T: DeserializeOwned>(root: &Path, relative: &str) -> Result<T, String> {
    let path = root.join(relative);
    let bytes = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    ron::de::from_bytes(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

fn nonempty(value: &str) -> bool {
    !value.trim().is_empty() && !value.contains("TBD") && !value.contains("TODO")
}

fn expect_counts(
    errors: &mut Vec<String>,
    actual: &BTreeMap<(String, String), usize>,
    visibility: &str,
    expected: &[(&'static str, &'static str, usize)],
) {
    for (domain, difficulty, count) in expected {
        let got = actual
            .get(&(String::from(*domain), String::from(*difficulty)))
            .copied()
            .unwrap_or(0);
        if got != *count {
            errors.push(format!(
                "{visibility} {domain}/{difficulty}: expected {count}, got {got}"
            ));
        }
    }
}

/// Validate the checked-in KAI-00 contract.
pub fn validate_repository(root: &Path) -> Result<ValidationSummary, Vec<String>> {
    let mut errors = Vec::new();
    let mut ids = BTreeSet::new();
    let mut public_inputs = BTreeSet::new();
    let mut public_counts = BTreeMap::new();
    let mut public_outcomes = Vec::new();

    for relative in PUBLIC_FILES {
        match load::<Corpus>(root, relative) {
            Ok(corpus) => {
                if corpus.version != 1 {
                    errors.push(format!("{relative}: version must be 1"));
                }
                let profiles: BTreeSet<_> = corpus
                    .acceptance_profiles
                    .iter()
                    .map(|p| p.id.as_str())
                    .collect();
                for profile in &corpus.acceptance_profiles {
                    if !nonempty(&profile.id)
                        || profile.invariants.is_empty()
                        || profile.journeys.is_empty()
                        || profile.quality_references.is_empty()
                        || profile.accessibility.is_empty()
                        || profile.budgets.is_empty()
                    {
                        errors.push(format!("{relative}: incomplete acceptance {}", profile.id));
                    }
                }
                for outcome in corpus.outcomes {
                    if !ids.insert(outcome.id.clone()) {
                        errors.push(format!("duplicate OutcomeId {}", outcome.id));
                    }
                    if !outcome.id.starts_with("KAI-V1-") || outcome.id.contains('/') {
                        errors.push(format!("invalid anti-chunking OutcomeId {}", outcome.id));
                    }
                    if !profiles.contains(outcome.acceptance.as_str()) {
                        errors.push(format!("{}: unknown acceptance profile", outcome.id));
                    }
                    if !nonempty(&outcome.input)
                        || outcome.maximum_scope.semantic_operations == 0
                        || outcome.maximum_scope.files == 0
                    {
                        errors.push(format!("{}: missing input or maximum scope", outcome.id));
                    }
                    if !public_inputs.insert(outcome.input.clone()) {
                        errors.push(format!("{}: duplicate public request", outcome.id));
                    }
                    for clarification in &outcome.clarifications {
                        if !nonempty(&clarification.id)
                            || !nonempty(&clarification.question)
                            || !nonempty(&clarification.answer)
                        {
                            errors.push(format!("{}: incomplete clarification", outcome.id));
                        }
                    }
                    *public_counts
                        .entry((outcome.domain.clone(), outcome.difficulty.clone()))
                        .or_insert(0) += 1;
                    public_outcomes.push(outcome);
                }
            }
            Err(e) => errors.push(e),
        }
    }
    expect_counts(
        &mut errors,
        &public_counts,
        "public",
        &[
            ("mechanic", "simple", 28),
            ("mechanic", "composed", 28),
            ("mechanic", "novel", 14),
            ("content", "simple", 56),
            ("content", "composed", 56),
            ("content", "novel", 28),
            ("asset", "simple", 14),
            ("asset", "composed", 14),
            ("asset", "novel", 7),
        ],
    );

    let held: Option<HeldOutCatalog> = match load(root, "benchmarks/kai/v1/held-out.hashes") {
        Ok(v) => Some(v),
        Err(e) => {
            errors.push(e);
            None
        }
    };
    let mut hidden_counts = BTreeMap::new();
    if let Some(held) = &held {
        let mut commitments = BTreeSet::new();
        if held.version != 1 || held.algorithm != "sha256" {
            errors.push("held-out catalog must use v1 sha256 commitments".into());
        }
        if !nonempty(&held.escrow.owner)
            || !nonempty(&held.escrow.custodian_role)
            || !nonempty(&held.escrow.receipt_id)
            || !nonempty(&held.escrow.location_class)
            || !nonempty(&held.escrow.rotation_policy)
        {
            errors.push("held-out escrow receipt is incomplete".into());
        }
        for entry in &held.entries {
            if !ids.insert(entry.id.clone()) {
                errors.push(format!("duplicate OutcomeId {}", entry.id));
            }
            if entry.prompt_sha256.len() != 64
                || !entry
                    .prompt_sha256
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            {
                errors.push(format!("{}: invalid sha256 commitment", entry.id));
            }
            if !commitments.insert(entry.prompt_sha256.as_str()) {
                errors.push(format!(
                    "{}: duplicate held-out prompt commitment",
                    entry.id
                ));
            }
            *hidden_counts
                .entry((entry.domain.clone(), entry.difficulty.clone()))
                .or_insert(0) += 1;
        }
    }
    expect_counts(
        &mut errors,
        &hidden_counts,
        "held-out",
        &[
            ("mechanic", "simple", 12),
            ("mechanic", "composed", 12),
            ("mechanic", "novel", 6),
            ("content", "simple", 24),
            ("content", "composed", 24),
            ("content", "novel", 12),
            ("asset", "simple", 6),
            ("asset", "composed", 6),
            ("asset", "novel", 3),
        ],
    );

    match load::<DifficultyCatalog>(root, "benchmarks/kai/v1/difficulty.ron") {
        Ok(catalog) => {
            if catalog.version != 1 || catalog.entries.len() != 350 {
                errors.push("difficulty catalog must contain exactly 350 v1 outcomes".into());
            }
            let entries: BTreeMap<_, _> = catalog
                .entries
                .iter()
                .map(|e| {
                    (
                        e.id.as_str(),
                        (
                            e.domain.as_str(),
                            e.difficulty.as_str(),
                            e.visibility.as_str(),
                        ),
                    )
                })
                .collect();
            if entries.len() != catalog.entries.len() {
                errors.push("difficulty catalog contains duplicate ids".into());
            }
            for outcome in &public_outcomes {
                if entries.get(outcome.id.as_str())
                    != Some(&(
                        outcome.domain.as_str(),
                        outcome.difficulty.as_str(),
                        "public",
                    ))
                {
                    errors.push(format!("{}: difficulty catalog drift", outcome.id));
                }
            }
            if let Some(held) = &held {
                for entry in &held.entries {
                    if entries.get(entry.id.as_str())
                        != Some(&(entry.domain.as_str(), entry.difficulty.as_str(), "held_out"))
                    {
                        errors.push(format!("{}: difficulty catalog drift", entry.id));
                    }
                }
            }
        }
        Err(e) => errors.push(e),
    }

    match load::<TimingContract>(root, "benchmarks/kai/v1/timing.ron") {
        Ok(timing) => {
            if timing.version != 1
                || timing.primary_clock != "monotonic_wall_clock"
                || timing.start_event != "request_submitted"
                || timing.stop_event != "evidence_complete_candidate_entered_review"
                || timing.public_trials < 5
                || timing.held_out_trials < 3
                || timing.order != "predeclared"
                || timing.never_paused_for.len() < 6
                || timing.separately_reported.len() < 6
                || timing
                    .required_metrics
                    .iter()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>()
                    != METRICS.into_iter().collect()
                || !nonempty(&timing.cold_cache.id)
                || !nonempty(&timing.cold_cache.derived_cache)
                || !nonempty(&timing.cold_cache.source_assets)
                || !nonempty(&timing.cold_cache.manifest_hash)
                || !nonempty(&timing.warm_cache.id)
                || !nonempty(&timing.warm_cache.derived_cache)
                || !nonempty(&timing.warm_cache.source_assets)
                || !nonempty(&timing.warm_cache.manifest_hash)
            {
                errors.push("timing contract is incomplete or weakens v1 clock rules".into());
            }
        }
        Err(e) => errors.push(e),
    }

    let machine_paths = ["ci/machines/kai-dev-a.ron", "ci/machines/kai-gpu-a.ron"];
    let mut machine_ids = BTreeSet::new();
    for path in machine_paths {
        match load::<MachineManifest>(root, path) {
            Ok(m) => {
                let fields = [
                    &m.id,
                    &m.owner,
                    &m.manufacturer,
                    &m.model,
                    &m.cpu,
                    &m.cpu_cores,
                    &m.microcode_or_firmware,
                    &m.ram,
                    &m.storage,
                    &m.filesystem,
                    &m.gpu,
                    &m.gpu_firmware,
                    &m.driver,
                    &m.api,
                    &m.os_image,
                    &m.kernel,
                    &m.compiler,
                    &m.display,
                    &m.controller,
                    &m.power_governor,
                    &m.thermal_precondition,
                    &m.process_affinity,
                    &m.claim_level,
                ];
                if m.version != 1 || m.polling_hz == 0 || fields.iter().any(|s| !nonempty(s)) {
                    errors.push(format!("{path}: incomplete machine manifest"));
                }
                machine_ids.insert(m.id);
            }
            Err(e) => errors.push(e),
        }
    }
    if machine_ids.len() != 2 {
        errors.push("machine ids must be unique".into());
    }

    match load::<FarmManifest>(root, "ci/farms/kai-farm-a.ron") {
        Ok(farm) => {
            let fields = [
                &farm.id,
                &farm.owner,
                &farm.queue_policy,
                &farm.cas_topology,
                &farm.cache_topology,
                &farm.storage_bandwidth,
                &farm.network_bandwidth,
                &farm.network_latency,
                &farm.retry_policy,
                &farm.scheduling,
            ];
            if farm.version != 1
                || farm.max_concurrency == 0
                || farm.measured_jobs_per_week == 0
                || fields.iter().any(|s| !nonempty(s))
                || farm.worker_classes.is_empty()
            {
                errors.push("farm manifest is incomplete".into());
            }
            for worker in farm.worker_classes {
                if !machine_ids.contains(&worker.machine_id)
                    || worker.count == 0
                    || worker.slots_each == 0
                {
                    errors.push(format!(
                        "farm references invalid worker {}",
                        worker.machine_id
                    ));
                }
            }
        }
        Err(e) => errors.push(e),
    }

    match load::<ModelLock>(root, "models/kai-benchmark.lock") {
        Ok(model) => {
            let fields = [
                &model.lane,
                &model.backend,
                &model.model_id,
                &model.artifact_path,
                &model.model_artifact_sha256,
                &model.sampling,
                &model.tool_catalog,
                &model.network_policy,
                &model.claim_level,
            ];
            if model.version != 1
                || model.context_tokens == 0
                || model.model_artifact_sha256.len() != 64
                || fields.iter().any(|s| !nonempty(s))
            {
                errors.push("model lock is incomplete".into());
            } else {
                match fs::read(root.join(&model.artifact_path)) {
                    Ok(bytes) => {
                        let actual = format!("{:x}", Sha256::digest(bytes));
                        if actual != model.model_artifact_sha256 {
                            errors.push("model artifact does not match its sha256 lock".into());
                        }
                    }
                    Err(e) => errors.push(format!("model artifact: {e}")),
                }
            }
        }
        Err(e) => errors.push(e),
    }

    match load::<FaultCatalog>(root, "benchmarks/kai/v1/faults/catalog.ron") {
        Ok(faults) => {
            let expected: BTreeSet<_> = [
                "contradiction",
                "cfg",
                "cap",
                "agency",
                "provenance",
                "package",
                "journey",
                "budget",
                "reproducibility",
            ]
            .into_iter()
            .collect();
            let actual: BTreeSet<_> = faults.entries.iter().map(|f| f.class.as_str()).collect();
            let fault_ids: BTreeSet<_> = faults.entries.iter().map(|f| f.id.as_str()).collect();
            if faults.version != 1 || actual != expected || fault_ids.len() != faults.entries.len()
            {
                errors.push("fault corpus does not cover every required diagnostic class".into());
            }
            for fault in faults.entries {
                if !fault.id.starts_with("KAI-V1-FAULT-")
                    || !nonempty(&fault.seed)
                    || !nonempty(&fault.expected_code)
                    || !nonempty(&fault.legal_repair)
                {
                    errors.push(format!("{}: incomplete fault fixture", fault.id));
                }
            }
        }
        Err(e) => errors.push(e),
    }

    let sku_paths = [
        "ci/skus/win-d3d12-high.ron",
        "ci/skus/linux-vulkan-high.ron",
        "ci/skus/mac-metal-high.ron",
        "ci/skus/desktop-minimum.ron",
    ];
    let mut sku_ids = BTreeSet::new();
    for path in sku_paths {
        match load::<SkuManifest>(root, path) {
            Ok(sku) => {
                let fields = [
                    &sku.id,
                    &sku.os,
                    &sku.graphics_api,
                    &sku.resolution,
                    &sku.quality,
                    &sku.controller,
                    &sku.package_claim_level,
                ];
                if sku.version != 1 || sku.refresh_hz == 0 || fields.iter().any(|s| !nonempty(s)) {
                    errors.push(format!("{path}: incomplete SKU manifest"));
                }
                let _vrr_is_explicit = sku.vrr;
                sku_ids.insert(sku.id);
            }
            Err(e) => errors.push(e),
        }
    }
    if sku_ids.len() != 4 {
        errors.push("SKU ids must be unique".into());
    }

    let mut planned_prs = 0;
    match load::<ProgramPlan>(root, "planning/kai-program.ron") {
        Ok(plan) => {
            if plan.version != 1
                || !nonempty(&plan.program_owner)
                || !nonempty(&plan.currency_overlay)
            {
                errors.push("program plan header is incomplete".into());
            }
            planned_prs = plan.entries.len();
            if planned_prs != 25 {
                errors.push(format!(
                    "program plan: expected 25 KAI entries, got {planned_prs}"
                ));
            }
            let mut plan_ids = BTreeSet::new();
            for entry in plan.entries {
                plan_ids.insert(entry.id.clone());
                if !nonempty(&entry.wave)
                    || !nonempty(&entry.accountable_owner)
                    || entry.critical_path_weeks == 0
                    || entry.contingency_percent == 0
                    || entry.roles.is_empty()
                    || entry.reviewer_hours_per_week == 0
                    || !nonempty(&entry.confidence)
                    || !nonempty(&entry.estimate_basis)
                    || !nonempty(&entry.actuals)
                    || !nonempty(&entry.reforecast_trigger)
                    || !nonempty(&entry.costs.redacted_range)
                {
                    errors.push(format!("{}: incomplete capacity entry", entry.id));
                }
                let _declared_cost_dimensions = (
                    entry.costs.model_tokens_million,
                    entry.costs.cpu_hours,
                    entry.costs.gpu_hours,
                    entry.costs.storage_gib,
                    entry.costs.egress_gib,
                    entry.costs.dcc_seats,
                    entry.costs.vendor_weeks,
                    entry.costs.device_lab_weeks,
                    entry.costs.devkits,
                    entry.costs.localization_words,
                    entry.costs.support_weeks,
                );
                let mut owner_load = BTreeMap::<&str, u32>::new();
                let mut owner_available = BTreeMap::<&str, u32>::new();
                for role in &entry.roles {
                    if !nonempty(&role.role) || !nonempty(&role.owner) {
                        errors.push(format!("{}: unnamed role capacity", entry.id));
                    }
                    if entry.wave == "foundation"
                        && (entry.funding != "approved"
                            || role.available_fte_weeks < role.estimated_fte_weeks)
                    {
                        errors.push(format!(
                            "{}: unfunded foundation role {}",
                            entry.id, role.role
                        ));
                    }
                    *owner_load.entry(&role.owner).or_default() +=
                        u32::from(role.estimated_fte_weeks);
                    *owner_available.entry(&role.owner).or_default() +=
                        u32::from(role.available_fte_weeks);
                }
                for (owner, load) in owner_load {
                    if load > u32::from(entry.critical_path_weeks) {
                        errors.push(format!(
                            "{}: {owner} has {load} sequential FTE-weeks on a {} week path",
                            entry.id, entry.critical_path_weeks
                        ));
                    }
                    if entry.wave == "foundation"
                        && owner_available.get(owner).copied().unwrap_or(0) < load
                    {
                        errors.push(format!("{}: {owner} lacks aggregate capacity", entry.id));
                    }
                }
                for queue in &entry.queues {
                    if !nonempty(&queue.queue)
                        || queue.service_per_week < queue.arrivals_per_week
                        || queue.maximum_backlog < queue.arrivals_per_week
                    {
                        errors.push(format!("{}: overloaded queue {}", entry.id, queue.queue));
                    }
                }
            }
            for ix in 0..=24 {
                let id = format!("KAI-{ix:02}");
                if !plan_ids.contains(&id) {
                    errors.push(format!("program plan missing {id}"));
                }
            }
        }
        Err(e) => errors.push(e),
    }

    if errors.is_empty() {
        let held_count = held.as_ref().map_or(0, |h| h.entries.len());
        Ok(ValidationSummary {
            public_outcomes: public_outcomes.len(),
            held_out_outcomes: held_count,
            total_outcomes: public_outcomes.len() + held_count,
            planned_prs,
        })
    } else {
        Err(errors)
    }
}

fn hash_file(root: &Path, path: &str) -> Result<String, String> {
    let bytes = fs::read(root.join(path)).map_err(|e| format!("{path}: {e}"))?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

/// Verify cleartext supplied by the independent benchmark owner without
/// persisting it in the repository or a command line.
pub fn verify_held_out_prompt(root: &Path, id: &str, prompt: &str) -> Result<(), String> {
    let held: HeldOutCatalog = load(root, "benchmarks/kai/v1/held-out.hashes")?;
    let entry = held
        .entries
        .iter()
        .find(|entry| entry.id == id)
        .ok_or_else(|| format!("unknown held-out OutcomeId {id}"))?;
    let actual = format!("{:x}", Sha256::digest(prompt.as_bytes()));
    if actual == entry.prompt_sha256 {
        Ok(())
    } else {
        Err(format!("{id}: prompt does not match escrow commitment"))
    }
}

/// Build a report that binds every advertised metric to all environment inputs.
pub fn build_report(root: &Path) -> Result<BenchmarkReport, Vec<String>> {
    let validation = validate_repository(root)?;
    let paths = [
        ("difficulty", "benchmarks/kai/v1/difficulty.ron"),
        ("held_out", "benchmarks/kai/v1/held-out.hashes"),
        ("governance", "benchmarks/kai/v1/GOVERNANCE.md"),
        ("timing", "benchmarks/kai/v1/timing.ron"),
        ("machine_dev", "ci/machines/kai-dev-a.ron"),
        ("machine_gpu", "ci/machines/kai-gpu-a.ron"),
        ("farm", "ci/farms/kai-farm-a.ron"),
        ("model", "models/kai-benchmark.lock"),
        ("program", "planning/kai-program.ron"),
        ("faults", "benchmarks/kai/v1/faults/catalog.ron"),
        ("sku_win", "ci/skus/win-d3d12-high.ron"),
        ("sku_linux", "ci/skus/linux-vulkan-high.ron"),
        ("sku_mac", "ci/skus/mac-metal-high.ron"),
        ("sku_minimum", "ci/skus/desktop-minimum.ron"),
        ("model_artifact", "models/fixtures/replay-v1.ron"),
    ];
    let mut hashes = BTreeMap::new();
    for (key, path) in paths {
        match hash_file(root, path) {
            Ok(hash) => {
                hashes.insert(key.into(), hash);
            }
            Err(e) => return Err(vec![e]),
        }
    }
    for path in PUBLIC_FILES {
        match hash_file(root, path) {
            Ok(hash) => {
                hashes.insert(path.into(), hash);
            }
            Err(e) => return Err(vec![e]),
        }
    }
    let generated_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    Ok(BenchmarkReport {
        schema: "klotho.kai.benchmark-report.v1".into(),
        generated_unix_ms,
        corpus_version: 1,
        environment_hashes: hashes,
        validation,
        metrics: METRICS
            .into_iter()
            .map(|metric| MetricReport {
                metric: metric.into(),
                status: "not_run".into(),
                sample_count: 0,
                p50: None,
                p95: None,
                unit: if metric.contains("latency") || metric == "engineer_burden" {
                    "milliseconds"
                } else {
                    "ratio"
                }
                .into(),
            })
            .collect(),
    })
}

/// Write a report atomically enough for the local harness (temporary sibling + rename).
pub fn write_report(root: &Path, output: &Path) -> Result<(), String> {
    let report = build_report(root).map_err(|e| e.join("\n"))?;
    let bytes = serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?;
    let temp = output.with_extension("tmp");
    fs::write(&temp, bytes).map_err(|e| e.to_string())?;
    fs::rename(&temp, output).map_err(|e| e.to_string())
}

/// Locate the repository when invoked through Cargo.
pub fn default_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate lives under workspace/crates")
        .to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_in_contract_is_complete() {
        let summary = validate_repository(&default_root()).expect("valid KAI-00 contract");
        assert_eq!(summary.public_outcomes, 245);
        assert_eq!(summary.held_out_outcomes, 105);
        assert_eq!(summary.total_outcomes, 350);
        assert_eq!(summary.planned_prs, 25);
    }

    #[test]
    fn report_emits_every_advertised_metric_and_environment_hash() {
        let report = build_report(&default_root()).expect("report");
        assert_eq!(report.metrics.len(), METRICS.len());
        assert!(report.metrics.iter().all(|m| m.status == "not_run"));
        assert_eq!(report.environment_hashes.len(), 18);
    }

    #[test]
    fn sha256_commitment_shape_rejects_uppercase_and_short_values() {
        let valid = "0123456789abcdef".repeat(4);
        assert_eq!(valid.len(), 64);
        assert!(
            valid
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        );
        let bad = "AB".repeat(32);
        assert!(
            !bad.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        );
    }

    #[test]
    fn held_out_promotion_requires_the_committed_prompt() {
        let prompt = "private fixture prompt";
        let expected = "d329b9a4e87d99510290d2707fced5aa7196809b104e41ed2191dd1afcadf6d6";
        assert_eq!(format!("{:x}", Sha256::digest(prompt.as_bytes())), expected);
        assert_ne!(format!("{:x}", Sha256::digest(b"changed prompt")), expected);

        let root =
            std::env::temp_dir().join(format!("klotho-kai-promotion-{}", std::process::id()));
        let dir = root.join("benchmarks/kai/v1");
        fs::create_dir_all(&dir).expect("temp benchmark directory");
        let catalog = format!(
            "(version:1,algorithm:\"sha256\",escrow:(owner:\"owner\",custodian_role:\"custodian\",receipt_id:\"receipt\",location_class:\"offline\",rotation_policy:\"major only\"),entries:[(id:\"KAI-V1-TEST-HOLD-001\",domain:\"mechanic\",difficulty:\"simple\",prompt_sha256:\"{expected}\")])"
        );
        fs::write(dir.join("held-out.hashes"), catalog).expect("temp catalog");
        assert!(verify_held_out_prompt(&root, "KAI-V1-TEST-HOLD-001", prompt).is_ok());
        assert!(verify_held_out_prompt(&root, "KAI-V1-TEST-HOLD-001", "changed").is_err());
        fs::remove_dir_all(root).expect("remove exact temp fixture");
    }
}
