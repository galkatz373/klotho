//! Shared authoring diagnostic envelope (K68 / KAI-04).
//!
//! Validators keep their native error types. This module is the machine-readable
//! form Distaff and later repair loops consume: a stable code, semantic anchors,
//! a minimal witness, legal repair shapes, and an optional cost. [`Display`] is
//! the concise human line; it is never a log dump.

use core::fmt;

use serde::{Deserialize, Serialize};

use klotho_prove::ProveError;

use crate::anchor::AnchorId;
use crate::error::IrError;

/// Domain mixed into blame anchors when a validator has a name but no
/// [`crate::ObjectAnchor`]. Never [`AnchorId::ZERO`].
const BLAME_DOMAIN: &[u8] = b"klotho-diag-v1";

/// Stable code cited by the schema catalog and the KAI-00 fault corpus.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DiagnosticCode(pub String);

impl DiagnosticCode {
    /// Contradiction / unsatisfiable Law fragment.
    pub const CONTRADICTION: &'static str = "KAI-DIAG-CONTRADICTION";
    /// Rite CFG: missing target, unreachable, cycle, fall-off.
    pub const CFG_TARGET: &'static str = "KAI-DIAG-CFG-TARGET";
    /// Pred or Rite step cap.
    pub const CAP: &'static str = "KAI-DIAG-CAP";
    /// Non-player construction of [`crate::Agency`], or duplicate claims.
    pub const AGENCY: &'static str = "KAI-DIAG-AGENCY";
    /// License / CAS / provenance graph.
    pub const PROVENANCE: &'static str = "KAI-DIAG-PROVENANCE";
    /// Ship package allowlist or warp container.
    pub const PACKAGE: &'static str = "KAI-DIAG-PACKAGE";
    /// Journey cannot reach its assertion.
    pub const JOURNEY: &'static str = "KAI-DIAG-JOURNEY";
    /// Budget miss with dominant blame.
    pub const BUDGET: &'static str = "KAI-DIAG-BUDGET";
    /// Content-hash drift under a locked environment.
    pub const HASH_DRIFT: &'static str = "KAI-DIAG-HASH-DRIFT";

    fn new(code: &str) -> Self {
        Self(code.to_owned())
    }
}

impl fmt::Display for DiagnosticCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Failure class. The KAI-00 fault corpus requires these names.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureClass {
    /// Unsatisfiable or pairwise-contradictory Laws.
    Contradiction,
    /// Rite control-flow.
    Cfg,
    /// Frozen pred/rite/header cap.
    Cap,
    /// Player-only Agency.
    Agency,
    /// Provenance / license / CAS.
    Provenance,
    /// Ship package / warp.
    Package,
    /// Playable journey.
    Journey,
    /// Sim/frame/memory budget.
    Budget,
    /// Hash / lock drift.
    Reproducibility,
    /// Schema, parse, or module structure. Not a KAI-00 seeded class.
    Schema,
}

impl FailureClass {
    /// Catalog / fault-corpus snake_case name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Contradiction => "contradiction",
            Self::Cfg => "cfg",
            Self::Cap => "cap",
            Self::Agency => "agency",
            Self::Provenance => "provenance",
            Self::Package => "package",
            Self::Journey => "journey",
            Self::Budget => "budget",
            Self::Reproducibility => "reproducibility",
            Self::Schema => "schema",
        }
    }
}

/// How loudly the failure is reported.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub enum Severity {
    /// Gate failure. Default for every seeded corpus class.
    Error,
    /// Non-blocking.
    Warning,
    /// Advisory critic finding. Never a merge gate.
    Advice,
}

/// Minimal witness. No secrets, no untrusted asset dumps.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Counterexample {
    /// Two (or one) Law ids whose `must` is unsatisfiable together.
    Contradiction {
        /// Law authoring ids, sorted.
        laws: Vec<String>,
    },
    /// Last reachable pc and the missing / blocked pc.
    Cfg {
        /// Last pc the CFG walker could stand on.
        last_reachable_pc: u16,
        /// Missing jump target or unreachable node.
        blocked_pc: u16,
    },
    /// Used versus frozen limit.
    Cap {
        /// Counted ops or steps.
        used: u32,
        /// Cap that rejected the input.
        limit: u32,
    },
    /// Who tried to claim a player-only channel.
    Agency {
        /// Attempted channel or `"any"`.
        channel: String,
        /// `player`, `mind`, `infer`, or `tool`.
        source: String,
    },
    /// License span and dependent CAS ids.
    Provenance {
        /// Escaped span description.
        span: String,
        /// Dependent blob or node ids, escaped.
        dependents: Vec<String>,
    },
    /// Package path and why it is forbidden.
    Package {
        /// Escaped path.
        path: String,
        /// Short reason.
        reason: String,
    },
    /// Last reachable semantic state and the affordance that blocked progress.
    Journey {
        /// Escaped last state.
        last_state: String,
        /// Blocked affordance id.
        blocked_affordance: String,
    },
    /// Dominant loci and the miss versus cap.
    Budget {
        /// Dominant locus / pattern names, most expensive first.
        dominant: Vec<String>,
        /// Observed cost.
        used: u32,
        /// Cap.
        cap: u32,
        /// Estimated savings if the dominant item is reduced.
        estimated_savings: u32,
    },
}

/// One legal authoring operation a repair loop may attempt.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepairShape {
    /// Catalog operation id (`author.remove@1`).
    pub op: String,
    /// Human/agent note. Matches the KAI-00 fault `legal_repair` where possible.
    pub note: String,
}

/// Static relative cost of applying the suggested repair.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EstimatedCost {
    /// Catalog `cost_units`.
    pub units: u16,
    /// Stage that produced the diagnostic (`validate`, `cook`, `package`, `prove`).
    pub stage: String,
}

/// Common diagnostic envelope. Native error [`Display`] is copied into
/// [`Self::message`] so human rendering stays a single short line.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    /// Stable code.
    pub code: DiagnosticCode,
    /// Fault class.
    pub class: FailureClass,
    /// Error / warning / advice.
    pub severity: Severity,
    /// Concise human line. Identical to the native error [`Display`] when wrapping one.
    pub message: String,
    /// Primary semantic anchor. [`AnchorId::ZERO`] only when no object exists.
    pub primary: AnchorId,
    /// Additional blamed objects.
    pub related: Vec<AnchorId>,
    /// Minimal counterexample.
    pub witness: Option<Counterexample>,
    /// Legal operations a bounded repair loop may apply.
    pub legal_repairs: Vec<RepairShape>,
    /// Optional repair cost.
    pub cost: Option<EstimatedCost>,
}

impl Diagnostic {
    /// `true` when [`Self::primary`] is a real semantic identity.
    #[must_use]
    pub fn points_to_anchor(&self) -> bool {
        self.primary != AnchorId::ZERO
    }

    /// Fraction of diagnostics whose primary is a semantic anchor.
    #[must_use]
    pub fn anchor_ratio(diags: &[Self]) -> (usize, usize) {
        let hit = diags.iter().filter(|d| d.points_to_anchor()).count();
        (hit, diags.len())
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl core::error::Error for Diagnostic {}

/// Catalog row generated into `klotho-schema`.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct DiagnosticCatalogEntry {
    /// Stable code.
    pub code: &'static str,
    /// Producing crate.
    pub source: &'static str,
    /// Fault class.
    pub class: FailureClass,
    /// Whether identical input may later succeed.
    pub retryable: bool,
    /// Legal repair notes.
    pub legal_repairs: &'static [&'static str],
}

/// Closed diagnostic surface. Schema CI fails if a code is dropped.
#[must_use]
pub fn diagnostic_catalog() -> &'static [DiagnosticCatalogEntry] {
    const IR: &[&str] = &[
        "IR.Parse",
        "IR.Ser",
        "IR.EmptyName",
        "IR.NestedQuantifier",
        "IR.InvalidRiteCap",
        "IR.InvalidPhase",
        "IR.DuplicateChannel",
        "IR.InvalidModuleVersion",
        "IR.ImportCycle",
        "IR.HashDrift",
        "IR.DuplicateModule",
        "IR.MissingModule",
        "IR.TombstoneReuse",
        "IR.AliasCollision",
        "IR.UnboundParameter",
        "IR.DuplicateAnchor",
        "IR.MissingAnchor",
        "IR.DuplicateObjectName",
        "IR.ExportUnknown",
        "IR.ParameterTypeMismatch",
        "IR.UnexpandedPattern",
        "IR.InvalidFeel",
        "IR.InvalidA11y",
        "IR.MindProgramCap",
        "IR.InvalidMindProgram",
        "IR.UnsafeFarFact",
    ];
    const PATTERN: &[&str] = &[
        "PATTERN.Unknown",
        "PATTERN.Version",
        "PATTERN.Arg",
        "PATTERN.Capability",
        "PATTERN.Conflict",
        "PATTERN.Budget",
    ];
    const EVAL: &[&str] = &[
        "EVAL.Stale",
        "EVAL.Select",
        "EVAL.Projection",
        "EVAL.Agency",
        "EVAL.Replay",
    ];
    const DIALOGUE: &[&str] = &[
        "DIALOGUE.Name",
        "DIALOGUE.Continuity",
        "DIALOGUE.Quest",
        "DIALOGUE.Line",
        "DIALOGUE.Loc",
        "DIALOGUE.Release",
        "DIALOGUE.Stale",
        "DIALOGUE.Replay",
    ];
    const CANON: &[&str] = &[
        "CANON.MixedLabeling",
        "CANON.DuplicatePc",
        "CANON.MissingEntry",
        "CANON.MissingTarget",
        "CANON.Unreachable",
        "CANON.FallOff",
        "CANON.Cycle",
        "CANON.UnboundName",
        "CANON.InvalidDoc",
        "CANON.DuplicateId",
        "CANON.UnknownRetract",
        "CANON.PredTooLarge",
        "CANON.TableFull",
        "CANON.Contradiction",
        "CANON.LockableNeedsKeyOrRite",
    ];
    const PROVE: &[&str] = &[
        "PROVE.UnknownLicense",
        "PROVE.InvalidLicense",
        "PROVE.MissingParent",
        "PROVE.MissingBlob",
        "PROVE.BlobTooLarge",
        "PROVE.CasFull",
    ];
    const COMPILE: &[&str] = &[
        "COMPILE.MissingTag",
        "COMPILE.LockMismatch",
        "COMPILE.MissingLockFile",
        "COMPILE.Catalog",
        "COMPILE.Header",
        "COMPILE.QuantizeOverflow",
        "COMPILE.Io",
        "COMPILE.Warp",
        "COMPILE.Gltf",
        "COMPILE.EpochOverflow",
        "COMPILE.Flatten",
        "COMPILE.PackageAllowlist",
    ];
    const SEEDED: &[DiagnosticCatalogEntry] = &[
        DiagnosticCatalogEntry {
            code: DiagnosticCode::CONTRADICTION,
            source: "klotho-canon",
            class: FailureClass::Contradiction,
            retryable: false,
            legal_repairs: &["remove one scoped fact"],
        },
        DiagnosticCatalogEntry {
            code: DiagnosticCode::CFG_TARGET,
            source: "klotho-canon",
            class: FailureClass::Cfg,
            retryable: false,
            legal_repairs: &["retarget within the owned Rite"],
        },
        DiagnosticCatalogEntry {
            code: DiagnosticCode::CAP,
            source: "klotho-compile",
            class: FailureClass::Cap,
            retryable: false,
            legal_repairs: &["reduce or split authored flow without changing OutcomeId"],
        },
        DiagnosticCatalogEntry {
            code: DiagnosticCode::AGENCY,
            source: "klotho-ir",
            class: FailureClass::Agency,
            retryable: false,
            legal_repairs: &["use public test-input adapter"],
        },
        DiagnosticCatalogEntry {
            code: DiagnosticCode::PROVENANCE,
            source: "klotho-prove",
            class: FailureClass::Provenance,
            retryable: false,
            legal_repairs: &["bind approved license evidence or remove blob"],
        },
        DiagnosticCatalogEntry {
            code: DiagnosticCode::PACKAGE,
            source: "klotho-compile",
            class: FailureClass::Package,
            retryable: false,
            legal_repairs: &["remove authoring artifact from package graph"],
        },
        DiagnosticCatalogEntry {
            code: DiagnosticCode::JOURNEY,
            source: "klotho-debug",
            class: FailureClass::Journey,
            retryable: false,
            legal_repairs: &["repair owned semantic operation from minimized witness"],
        },
        DiagnosticCatalogEntry {
            code: DiagnosticCode::BUDGET,
            source: "klotho-debug",
            class: FailureClass::Budget,
            retryable: false,
            legal_repairs: &["reduce scoped cost without weakening budget"],
        },
        DiagnosticCatalogEntry {
            code: DiagnosticCode::HASH_DRIFT,
            source: "klotho-ir",
            class: FailureClass::Reproducibility,
            retryable: false,
            legal_repairs: &["reject candidate and isolate nondeterministic input"],
        },
    ];

    // Leak a one-time concatenation so callers get a single slice. Catalog
    // generation runs once per process; the leak is the closed code list.
    use std::sync::OnceLock;
    static ALL: OnceLock<Vec<DiagnosticCatalogEntry>> = OnceLock::new();
    ALL.get_or_init(|| {
        const NONE: &[&str] = &[];
        let mut out = Vec::from(SEEDED);
        for code in IR {
            out.push(DiagnosticCatalogEntry {
                code,
                source: "klotho-ir",
                class: class_for_detail(code),
                retryable: false,
                legal_repairs: NONE,
            });
        }
        for code in CANON {
            out.push(DiagnosticCatalogEntry {
                code,
                source: "klotho-canon",
                class: class_for_detail(code),
                retryable: false,
                legal_repairs: NONE,
            });
        }
        for code in PROVE {
            out.push(DiagnosticCatalogEntry {
                code,
                source: "klotho-prove",
                class: FailureClass::Provenance,
                retryable: false,
                legal_repairs: NONE,
            });
        }
        for code in COMPILE {
            out.push(DiagnosticCatalogEntry {
                code,
                source: "klotho-compile",
                class: class_for_detail(code),
                retryable: false,
                legal_repairs: NONE,
            });
        }
        for code in PATTERN {
            out.push(DiagnosticCatalogEntry {
                code,
                source: "klotho-pattern",
                class: class_for_detail(code),
                retryable: false,
                legal_repairs: NONE,
            });
        }
        for code in EVAL {
            out.push(DiagnosticCatalogEntry {
                code,
                source: "klotho-eval",
                class: class_for_detail(code),
                retryable: false,
                legal_repairs: NONE,
            });
        }
        for code in DIALOGUE {
            out.push(DiagnosticCatalogEntry {
                code,
                source: "klotho-dialogue",
                class: class_for_detail(code),
                retryable: false,
                legal_repairs: NONE,
            });
        }
        out.push(DiagnosticCatalogEntry {
            code: "DEBUG.Budget",
            source: "klotho-debug",
            class: FailureClass::Budget,
            retryable: true,
            legal_repairs: NONE,
        });
        out
    })
}

fn class_for_detail(code: &str) -> FailureClass {
    match code {
        "IR.DuplicateChannel" => FailureClass::Agency,
        "IR.HashDrift" | "COMPILE.LockMismatch" | "COMPILE.Flatten" => {
            FailureClass::Reproducibility
        }
        "IR.InvalidRiteCap" | "IR.MindProgramCap" | "CANON.PredTooLarge" | "CANON.TableFull"
        | "COMPILE.Header" => FailureClass::Cap,
        "CANON.MixedLabeling"
        | "CANON.DuplicatePc"
        | "CANON.MissingEntry"
        | "CANON.MissingTarget"
        | "CANON.Unreachable"
        | "CANON.FallOff"
        | "CANON.Cycle" => FailureClass::Cfg,
        "CANON.Contradiction" | "CANON.LockableNeedsKeyOrRite" => FailureClass::Contradiction,
        "PATTERN.Budget" => FailureClass::Budget,
        "EVAL.Stale" | "DIALOGUE.Stale" => FailureClass::Reproducibility,
        "EVAL.Select" | "EVAL.Replay" | "DIALOGUE.Replay" => FailureClass::Journey,
        "EVAL.Projection" | "EVAL.Agency" => FailureClass::Agency,
        "DIALOGUE.Continuity" | "DIALOGUE.Quest" => FailureClass::Contradiction,
        "DIALOGUE.Release" => FailureClass::Provenance,
        "COMPILE.Warp" | "COMPILE.PackageAllowlist" | "COMPILE.MissingLockFile" => {
            FailureClass::Package
        }
        _ => FailureClass::Schema,
    }
}

/// Deterministic blame identity from a kind and token.
#[must_use]
pub fn blame_anchor(kind: &str, token: &str) -> AnchorId {
    let mut token_bytes = Vec::with_capacity(kind.len() + 1 + token.len());
    token_bytes.extend_from_slice(kind.as_bytes());
    token_bytes.push(b':');
    token_bytes.extend_from_slice(token.as_bytes());
    AnchorId::derive(BLAME_DOMAIN, &token_bytes)
}

/// Label untrusted or external text as data. Never a model instruction.
#[must_use]
pub fn as_data(text: &str) -> String {
    let mut escaped = String::from("data:\"");
    for c in text.chars() {
        match c {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            c if c.is_control() => escaped.push('?'),
            c => escaped.push(c),
        }
    }
    escaped.push('"');
    escaped
}

fn repair(op: &str, note: &str) -> RepairShape {
    RepairShape {
        op: op.to_owned(),
        note: note.to_owned(),
    }
}

fn cost(units: u16, stage: &str) -> EstimatedCost {
    EstimatedCost {
        units,
        stage: stage.to_owned(),
    }
}

#[allow(clippy::too_many_arguments)]
fn envelope(
    code: &str,
    class: FailureClass,
    message: String,
    primary: AnchorId,
    related: Vec<AnchorId>,
    witness: Option<Counterexample>,
    legal_repairs: Vec<RepairShape>,
    cost: Option<EstimatedCost>,
) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::new(code),
        class,
        severity: Severity::Error,
        message,
        primary,
        related,
        witness,
        legal_repairs,
        cost,
    }
}

/// Contradiction: one or two Law ids.
#[must_use]
pub fn diagnose_contradiction(laws: &[&str], message: impl Into<String>) -> Diagnostic {
    let mut names: Vec<String> = laws.iter().map(|s| (*s).to_owned()).collect();
    names.sort();
    names.dedup();
    let primary = names
        .first()
        .map(|n| blame_anchor("law", n))
        .unwrap_or(AnchorId::ZERO);
    let related = names
        .iter()
        .skip(1)
        .map(|n| blame_anchor("law", n))
        .collect();
    envelope(
        DiagnosticCode::CONTRADICTION,
        FailureClass::Contradiction,
        message.into(),
        primary,
        related,
        Some(Counterexample::Contradiction {
            laws: names.clone(),
        }),
        vec![repair("author.remove@1", "remove one scoped fact")],
        Some(cost(3, "cook")),
    )
}

/// Rite CFG failure.
#[must_use]
pub fn diagnose_cfg(
    rite: &str,
    last_reachable_pc: u16,
    blocked_pc: u16,
    message: impl Into<String>,
) -> Diagnostic {
    envelope(
        DiagnosticCode::CFG_TARGET,
        FailureClass::Cfg,
        message.into(),
        blame_anchor("rite", rite),
        vec![blame_anchor("rite-pc", &blocked_pc.to_string())],
        Some(Counterexample::Cfg {
            last_reachable_pc,
            blocked_pc,
        }),
        vec![repair("canon.diff.add@1", "retarget within the owned Rite")],
        Some(cost(2, "cook")),
    )
}

/// Pred / Rite / header cap.
#[must_use]
pub fn diagnose_cap(
    subject: &str,
    used: u32,
    limit: u32,
    message: impl Into<String>,
) -> Diagnostic {
    envelope(
        DiagnosticCode::CAP,
        FailureClass::Cap,
        message.into(),
        blame_anchor("cap", subject),
        Vec::new(),
        Some(Counterexample::Cap { used, limit }),
        vec![repair(
            "author.set_argument@1",
            "reduce or split authored flow without changing OutcomeId",
        )],
        Some(cost(1, "cook")),
    )
}

/// Non-player Agency construction, or duplicate player claims.
#[must_use]
pub fn diagnose_agency(source: &str, channel: &str, message: impl Into<String>) -> Diagnostic {
    envelope(
        DiagnosticCode::AGENCY,
        FailureClass::Agency,
        message.into(),
        blame_anchor("agency", source),
        Vec::new(),
        Some(Counterexample::Agency {
            channel: channel.to_owned(),
            source: source.to_owned(),
        }),
        vec![repair(
            "author.add_journey@1",
            "use public test-input adapter",
        )],
        Some(cost(3, "validate")),
    )
}

/// Provenance / license / CAS.
#[must_use]
pub fn diagnose_provenance(
    span: &str,
    dependents: &[&str],
    message: impl Into<String>,
) -> Diagnostic {
    let deps: Vec<String> = dependents.iter().map(|d| as_data(d)).collect();
    envelope(
        DiagnosticCode::PROVENANCE,
        FailureClass::Provenance,
        message.into(),
        blame_anchor("provenance", span),
        Vec::new(),
        Some(Counterexample::Provenance {
            span: as_data(span),
            dependents: deps,
        }),
        vec![repair(
            "reference.replace@1",
            "bind approved license evidence or remove blob",
        )],
        Some(cost(2, "prove")),
    )
}

/// Package allowlist / warp.
#[must_use]
pub fn diagnose_package(path: &str, reason: &str, message: impl Into<String>) -> Diagnostic {
    envelope(
        DiagnosticCode::PACKAGE,
        FailureClass::Package,
        message.into(),
        blame_anchor("package", path),
        Vec::new(),
        Some(Counterexample::Package {
            path: as_data(path),
            reason: reason.to_owned(),
        }),
        vec![repair(
            "author.remove@1",
            "remove authoring artifact from package graph",
        )],
        Some(cost(3, "package")),
    )
}

/// Unreachable journey. The runner lands in KAI-06; the envelope is KAI-04.
#[must_use]
pub fn diagnose_journey(
    journey: &str,
    last_state: &str,
    blocked_affordance: &str,
    message: impl Into<String>,
) -> Diagnostic {
    envelope(
        DiagnosticCode::JOURNEY,
        FailureClass::Journey,
        message.into(),
        blame_anchor("journey", journey),
        vec![blame_anchor("affordance", blocked_affordance)],
        Some(Counterexample::Journey {
            last_state: as_data(last_state),
            blocked_affordance: blocked_affordance.to_owned(),
        }),
        vec![repair(
            "author.add_fact@1",
            "repair owned semantic operation from minimized witness",
        )],
        Some(cost(1, "eval")),
    )
}

/// Budget miss with dominant blame.
#[must_use]
pub fn diagnose_budget(
    subject: &str,
    dominant: &[&str],
    used: u32,
    cap: u32,
    message: impl Into<String>,
) -> Diagnostic {
    let savings = used.saturating_sub(cap);
    let related = dominant.iter().map(|n| blame_anchor("locus", n)).collect();
    envelope(
        DiagnosticCode::BUDGET,
        FailureClass::Budget,
        message.into(),
        blame_anchor("budget", subject),
        related,
        Some(Counterexample::Budget {
            dominant: dominant.iter().map(|s| (*s).to_owned()).collect(),
            used,
            cap,
            estimated_savings: savings,
        }),
        vec![repair(
            "author.remove@1",
            "reduce scoped cost without weakening budget",
        )],
        Some(cost(3, "eval")),
    )
}

/// Hash / lock drift.
#[must_use]
pub fn diagnose_hash_drift(
    id: &str,
    expected: &str,
    actual: &str,
    message: impl Into<String>,
) -> Diagnostic {
    envelope(
        DiagnosticCode::HASH_DRIFT,
        FailureClass::Reproducibility,
        message.into(),
        blame_anchor("module", id),
        Vec::new(),
        Some(Counterexample::Provenance {
            span: as_data(id),
            dependents: vec![as_data(expected), as_data(actual)],
        }),
        vec![repair(
            "module.import@1",
            "reject candidate and isolate nondeterministic input",
        )],
        Some(cost(2, "validate")),
    )
}

/// Named schema/module failure with a blame token.
#[must_use]
pub fn diagnose_named(
    code: &str,
    class: FailureClass,
    kind: &str,
    token: &str,
    message: impl Into<String>,
) -> Diagnostic {
    let primary = if token.is_empty() {
        AnchorId::ZERO
    } else {
        blame_anchor(kind, token)
    };
    envelope(
        code,
        class,
        message.into(),
        primary,
        Vec::new(),
        None,
        Vec::new(),
        Some(cost(1, "validate")),
    )
}

fn schema_diag(code: &str, token: &str, message: String) -> Diagnostic {
    let primary = if token.is_empty() {
        AnchorId::ZERO
    } else {
        blame_anchor("ir", token)
    };
    envelope(
        code,
        FailureClass::Schema,
        message,
        primary,
        Vec::new(),
        None,
        Vec::new(),
        Some(cost(1, "validate")),
    )
}

impl IrError {
    /// Shared envelope. [`Display`] of `self` is preserved as the message.
    #[must_use]
    pub fn to_diagnostic(&self) -> Diagnostic {
        let message = self.to_string();
        match self {
            Self::Parse(_) => schema_diag("IR.Parse", "", message),
            Self::Ser(_) => schema_diag("IR.Ser", "", message),
            Self::EmptyName => schema_diag("IR.EmptyName", "empty", message),
            Self::NestedQuantifier => schema_diag("IR.NestedQuantifier", "quantifier", message),
            Self::InvalidRiteCap => diagnose_cap("rite", 0, 1, message),
            Self::InvalidPhase(_) => schema_diag("IR.InvalidPhase", "phase", message),
            Self::DuplicateChannel => diagnose_agency("player", "duplicate", message),
            Self::InvalidModuleVersion => schema_diag("IR.InvalidModuleVersion", "module", message),
            Self::ImportCycle(ids) => {
                let token = ids.first().map(String::as_str).unwrap_or("cycle");
                schema_diag("IR.ImportCycle", token, message)
            }
            Self::HashDrift {
                id,
                expected,
                actual,
            } => diagnose_hash_drift(id, expected, actual, message),
            Self::DuplicateModule(id) => schema_diag("IR.DuplicateModule", id, message),
            Self::MissingModule(id) => schema_diag("IR.MissingModule", id, message),
            Self::TombstoneReuse(name) => schema_diag("IR.TombstoneReuse", name, message),
            Self::AliasCollision(name) => schema_diag("IR.AliasCollision", name, message),
            Self::UnboundParameter(name) => schema_diag("IR.UnboundParameter", name, message),
            Self::DuplicateAnchor(id) => schema_diag("IR.DuplicateAnchor", id, message),
            Self::MissingAnchor(name) => schema_diag("IR.MissingAnchor", name, message),
            Self::DuplicateObjectName(name) => schema_diag("IR.DuplicateObjectName", name, message),
            Self::ExportUnknown(name) => schema_diag("IR.ExportUnknown", name, message),
            Self::ParameterTypeMismatch(name) => {
                schema_diag("IR.ParameterTypeMismatch", name, message)
            }
            Self::UnexpandedPattern(id) => schema_diag("IR.UnexpandedPattern", id, message),
            Self::InvalidFeel { field, .. } => schema_diag("IR.InvalidFeel", field, message),
            Self::InvalidA11y { field, .. } => schema_diag("IR.InvalidA11y", field, message),
            Self::MindProgramCap {
                locus, actual, cap, ..
            } => diagnose_cap(locus, *actual as u32, *cap as u32, message),
            Self::InvalidMindProgram(reason) => {
                schema_diag("IR.InvalidMindProgram", reason, message)
            }
            Self::UnsafeFarFact { locus, .. } => schema_diag("IR.UnsafeFarFact", locus, message),
        }
    }
}

impl From<&IrError> for Diagnostic {
    fn from(e: &IrError) -> Self {
        e.to_diagnostic()
    }
}

impl From<&ProveError> for Diagnostic {
    fn from(e: &ProveError) -> Self {
        prove_to_diagnostic(e)
    }
}

/// Provenance envelope. `klotho-prove` does not depend on this crate.
#[must_use]
pub fn prove_to_diagnostic(e: &ProveError) -> Diagnostic {
    let message = e.to_string();
    match e {
        ProveError::UnknownLicense => diagnose_provenance("Unknown", &[], message),
        ProveError::InvalidLicense => diagnose_provenance("Invalid", &[], message),
        ProveError::MissingParent(id) => {
            diagnose_provenance("MissingParent", &[&id.to_string()], message)
        }
        ProveError::MissingBlob(id) => {
            diagnose_provenance("MissingBlob", &[&id.to_string()], message)
        }
        ProveError::BlobTooLarge { size } => {
            diagnose_provenance("BlobTooLarge", &[&size.to_string()], message)
        }
        ProveError::CasFull => diagnose_provenance("CasFull", &[], message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{from_ron, to_ron};

    #[test]
    fn blame_anchor_is_stable_and_not_zero() {
        let a = blame_anchor("law", "alive");
        assert_eq!(a, blame_anchor("law", "alive"));
        assert_ne!(a, AnchorId::ZERO);
        assert_ne!(a, blame_anchor("law", "dead"));
    }

    #[test]
    fn display_is_the_concise_native_line() {
        let err = IrError::DuplicateChannel;
        let diag = err.to_diagnostic();
        assert_eq!(diag.to_string(), err.to_string());
        assert_eq!(diag.to_string(), "DuplicateChannel");
        assert_eq!(diag.code.0, DiagnosticCode::AGENCY);
        assert!(diag.points_to_anchor());
        assert!(diag.witness.is_some());
        assert!(!diag.legal_repairs.is_empty());
    }

    #[test]
    fn as_data_escapes_quotes_and_control() {
        assert_eq!(as_data("ok"), "data:\"ok\"");
        assert_eq!(as_data("a\"b\\c"), "data:\"a\\\"b\\\\c\"");
        assert!(as_data("line\nfeed").contains("\\n"));
        assert!(!as_data("secret").contains("instruction"));
    }

    #[test]
    fn envelope_round_trips_ron() {
        let d = diagnose_contradiction(&["alive", "dead"], "Contradiction(alive|dead)");
        let text = to_ron(&d).unwrap();
        let back: Diagnostic = from_ron(&text).unwrap();
        assert_eq!(d, back);
        assert!(!text.contains("<<<<<<"));
    }

    #[test]
    fn catalog_contains_every_seeded_code() {
        let codes: Vec<_> = diagnostic_catalog().iter().map(|e| e.code).collect();
        for needed in [
            DiagnosticCode::CONTRADICTION,
            DiagnosticCode::CFG_TARGET,
            DiagnosticCode::CAP,
            DiagnosticCode::AGENCY,
            DiagnosticCode::PROVENANCE,
            DiagnosticCode::PACKAGE,
            DiagnosticCode::JOURNEY,
            DiagnosticCode::BUDGET,
            DiagnosticCode::HASH_DRIFT,
        ] {
            assert!(codes.contains(&needed), "missing {needed}");
        }
    }

    #[test]
    fn prove_unknown_license_is_provenance() {
        let d = prove_to_diagnostic(&ProveError::UnknownLicense);
        assert_eq!(d.code.0, DiagnosticCode::PROVENANCE);
        assert_eq!(d.message, "UnknownLicense");
        assert!(d.points_to_anchor());
    }

    #[test]
    fn hash_drift_keeps_ids_out_of_instructions() {
        let d = IrError::HashDrift {
            id: "main".into(),
            expected: "aa".into(),
            actual: "bb".into(),
        }
        .to_diagnostic();
        assert_eq!(d.code.0, DiagnosticCode::HASH_DRIFT);
        match d.witness {
            Some(Counterexample::Provenance { span, .. }) => {
                assert!(span.starts_with("data:"));
            }
            other => panic!("{other:?}"),
        }
    }
}
