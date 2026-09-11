//! Stable, generated discovery catalog for Klotho authoring.
//!
//! Validators and the Canon cook remain authoritative. This catalog describes
//! their closed input surface for Distaff and later Klotho AI tooling.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::fs;
use std::path::Path;

use klotho_canon::Canon;
use klotho_core::{Budget, Hash, LocusKind, SimLod};
use klotho_ir::{Rel, Verb};
use serde::{Deserialize, Serialize};

/// Current catalog format. A breaking schema change increments this value.
pub const SCHEMA_VERSION: u32 = 1;

/// Complete discovery catalog for public authoring tools.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaCatalog {
    /// Catalog format version.
    pub version: u32,
    /// Hash of every catalog entry except this field.
    pub toolchain_hash: Hash,
    /// Public authoring data types.
    pub kinds: Vec<TypeSchema>,
    /// Stable runtime verbs.
    pub verbs: Vec<TagSchema>,
    /// Stable relation labels.
    pub rels: Vec<TagSchema>,
    /// Closed predicate variants.
    pub predicates: Vec<VariantSchema>,
    /// Closed Rite instruction variants.
    pub rite_ops: Vec<VariantSchema>,
    /// Project-specific affordances read from cooked Canon.
    pub affordances: Vec<AffordanceSchema>,
    /// Pattern versions. Empty until KAI-05.
    pub patterns: Vec<PatternSchema>,
    /// Stable validator and cook diagnostic codes.
    pub diagnostics: Vec<DiagnosticSchema>,
    /// Named deterministic cap and telemetry profiles.
    pub budgets: Vec<BudgetSchema>,
    /// Semantic operation forms exposed by the authoring protocol.
    pub operations: Vec<OperationSchema>,
}

/// One authorable Rust type.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypeSchema {
    /// Stable catalog id.
    pub id: String,
    /// Struct, enum, scalar, or transparent wrapper.
    pub shape: String,
    /// Named fields for structs.
    pub fields: Vec<FieldSchema>,
    /// Named variants for enums.
    pub variants: Vec<VariantSchema>,
    /// Positive canonical RON example.
    pub positive_example: String,
    /// Invalid or rejected example.
    pub negative_example: String,
}

/// One named struct field.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldSchema {
    /// Serialized field name.
    pub name: String,
    /// Stable Rust-facing type expression.
    pub ty: String,
    /// Human-facing meaning or unit.
    pub description: String,
    /// Optional inclusive lower bound.
    pub min: Option<i64>,
    /// Optional inclusive upper bound.
    pub max: Option<i64>,
}

/// One enum or instruction variant.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariantSchema {
    /// Stable serialized name.
    pub name: String,
    /// Frozen discriminant where the format has one.
    pub discriminant: Option<u16>,
    /// Serialized payload shape.
    pub payload: String,
    /// Human-facing meaning.
    pub description: String,
}

/// A stable unit tag and discriminant.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TagSchema {
    /// Serialized name.
    pub name: String,
    /// Frozen wire/catalog value.
    pub discriminant: u16,
}

/// A capability compiled into Canon.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AffordanceSchema {
    /// Packed id in this Canon.
    pub id: u16,
    /// Stable authoring name.
    pub name: String,
    /// Number of compiled requirements.
    pub requires: u16,
    /// Granted verb or Rite tags.
    pub grants: Vec<String>,
    /// Conflicting packed ids.
    pub conflicts: Vec<u16>,
}

/// Registered reusable pattern. Populated by KAI-05.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatternSchema {
    /// Stable pattern id.
    pub id: String,
    /// Append-only pattern version.
    pub version: u32,
}

/// Stable diagnostic discovery entry.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticSchema {
    /// Stable namespaced code.
    pub code: String,
    /// Producing subsystem.
    pub source: String,
    /// Fault class (`contradiction`, `cfg`, …).
    pub class: String,
    /// Whether retrying identical input can succeed.
    pub retryable: bool,
    /// Legal repair notes a bounded loop may attempt.
    pub legal_repairs: Vec<String>,
}

/// Named deterministic budget profile.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetSchema {
    /// Stable profile name.
    pub name: String,
    /// Kernel admit telemetry target in microseconds.
    pub us_sim: u32,
    /// Predicate op cap.
    pub pred_ops: u32,
    /// Rite instruction cap.
    pub rite_steps: u32,
    /// Infer evaluation SLO in ticks.
    pub eval_slo_ticks: u16,
    /// Rewind cap in ticks.
    pub rewind_ticks: u16,
}

/// Semantic authoring operation descriptor. Implementations land after KAI-01.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationSchema {
    /// Stable operation id and version.
    pub id: String,
    /// Input type or parameter tuple.
    pub input: String,
    /// Objects the operation may write.
    pub writes: Vec<String>,
    /// Static relative cost units for planning.
    pub cost_units: u16,
}

/// Generate the catalog from public Rust declarations and a cooked Canon.
#[must_use]
pub fn generate(canon: &Canon) -> SchemaCatalog {
    let mut catalog = SchemaCatalog {
        version: SCHEMA_VERSION,
        toolchain_hash: Hash::ZERO,
        kinds: type_schemas(),
        verbs: verbs(),
        rels: rels(),
        predicates: predicates(),
        rite_ops: rite_ops(),
        affordances: canon
            .affordances
            .iter()
            .map(|a| AffordanceSchema {
                id: a.id.0,
                name: a.name.as_str().to_owned(),
                requires: u16::try_from(a.requires.len()).unwrap_or(u16::MAX),
                grants: a
                    .grants
                    .iter()
                    .map(|name| name.as_str().to_owned())
                    .collect(),
                conflicts: a.conflicts.iter().map(|id| id.0).collect(),
            })
            .collect(),
        patterns: Vec::new(),
        diagnostics: diagnostics(),
        budgets: budgets(),
        operations: operations(),
    };
    catalog.affordances.sort_by(|a, b| a.name.cmp(&b.name));
    let bytes = serde_json::to_vec(&catalog).expect("SchemaCatalog always serializes");
    catalog.toolchain_hash = Hash::from_bytes(*blake3::hash(&bytes).as_bytes());
    catalog
}

/// Serialize a catalog in canonical pretty JSON with one trailing newline.
pub fn to_pretty_json(catalog: &SchemaCatalog) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(catalog).map(|mut json| {
        json.push('\n');
        json
    })
}

/// Compare a generated catalog with the checked-in compatibility golden.
pub fn check_golden(catalog: &SchemaCatalog, path: &Path) -> Result<(), String> {
    let expected = fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let actual = to_pretty_json(catalog).map_err(|error| error.to_string())?;
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "{} is stale; review the compatibility change and regenerate with `cargo run --manifest-path studio/Cargo.toml -p klotho-schema -- print`",
            path.display()
        ))
    }
}

fn field(name: &str, ty: &str, description: &str) -> FieldSchema {
    FieldSchema {
        name: name.to_owned(),
        ty: ty.to_owned(),
        description: description.to_owned(),
        min: None,
        max: None,
    }
}

fn bounded(name: &str, ty: &str, description: &str, min: i64, max: i64) -> FieldSchema {
    FieldSchema {
        name: name.to_owned(),
        ty: ty.to_owned(),
        description: description.to_owned(),
        min: Some(min),
        max: Some(max),
    }
}

fn structure(id: &str, fields: Vec<FieldSchema>, positive: &str, negative: &str) -> TypeSchema {
    TypeSchema {
        id: id.to_owned(),
        shape: "struct".to_owned(),
        fields,
        variants: Vec::new(),
        positive_example: positive.to_owned(),
        negative_example: negative.to_owned(),
    }
}

fn enumeration(id: &str, variants: Vec<VariantSchema>, positive: &str) -> TypeSchema {
    TypeSchema {
        id: id.to_owned(),
        shape: "enum".to_owned(),
        fields: Vec::new(),
        variants,
        positive_example: positive.to_owned(),
        negative_example: "UnknownVariant".to_owned(),
    }
}

fn variant(
    name: &str,
    discriminant: Option<u16>,
    payload: &str,
    description: &str,
) -> VariantSchema {
    VariantSchema {
        name: name.to_owned(),
        discriminant,
        payload: payload.to_owned(),
        description: description.to_owned(),
    }
}

fn type_schemas() -> Vec<TypeSchema> {
    vec![
        structure(
            "klotho_ir::IntentDoc",
            vec![
                field("style", "StyleIntent", "presentation hints"),
                field("canon_diffs", "Vec<CanonDiff>", "Canon patches"),
                field("seed", "Vec<SeedFact>", "seed Trace facts"),
                field("minds", "Vec<MindSpec>", "NPC specifications"),
                field("provenance", "ProvenanceId", "document provenance root"),
            ],
            "(style:(notes:\"\",palettes:[],kitbash_tags:[]),canon_diffs:[],seed:[],minds:[],provenance:\"0000000000000000000000000000000000000000000000000000000000000000\")",
            "(style:())",
        ),
        structure(
            "klotho_ir::IntentProject",
            vec![
                field("project", "Name", "project namespace"),
                field(
                    "modules",
                    "Vec<IntentModuleRef>",
                    "module refs; flatten sorts by id",
                ),
                field("lock", "ModuleLock", "content-hash lock"),
            ],
            "(project:\"hearth\",modules:[],lock:(entries:[]))",
            "(project:\"\",modules:[],lock:(entries:[]))",
        ),
        structure(
            "klotho_ir::IntentModule",
            vec![
                field("anchor", "AnchorId", "immutable module identity"),
                field("id", "Name", "authoring id"),
                field("version", "u32", "module format version"),
                field("imports", "Vec<ModuleImport>", "hash-locked imports"),
                field("exports", "Vec<Name>", "exported names"),
                field("parameters", "Vec<ParameterDecl>", "module parameters"),
                field("aliases", "Vec<NameAlias>", "former names"),
                field("tombstones", "Vec<Tombstone>", "removed objects"),
                field("object_anchors", "Vec<ObjectAnchor>", "frozen identities"),
                field("body", "IntentDoc", "ordinary Intent body"),
            ],
            "(anchor:\"00000000000000000000000000000000\",id:\"main\",version:1,imports:[],exports:[],parameters:[],aliases:[],tombstones:[],object_anchors:[],body:(style:(notes:\"\",palettes:[],kitbash_tags:[]),canon_diffs:[],seed:[],minds:[],provenance:\"0000000000000000000000000000000000000000000000000000000000000000\"))",
            "(anchor:\"00\",id:\"\",version:0,imports:[],exports:[],parameters:[],aliases:[],tombstones:[],object_anchors:[],body:())",
        ),
        structure(
            "klotho_ir::IntentModuleRef",
            vec![
                field("id", "Name", "module id"),
                field("path", "String", "path relative to the project file"),
                field("hash", "Hash", "content hash"),
            ],
            "(id:\"main\",path:\"modules/main.ron\",hash:\"0000000000000000000000000000000000000000000000000000000000000000\")",
            "(id:\"\",path:\"\",hash:\"00\")",
        ),
        structure(
            "klotho_ir::ModuleLock",
            vec![field("entries", "Vec<LockEntry>", "sorted id/hash pairs")],
            "(entries:[])",
            "(entries:())",
        ),
        structure(
            "klotho_ir::ModuleImport",
            vec![
                field("id", "Name", "imported module id"),
                field("hash", "Hash", "expected content hash"),
            ],
            "(id:\"core\",hash:\"0000000000000000000000000000000000000000000000000000000000000000\")",
            "(id:\"\",hash:\"00\")",
        ),
        structure(
            "klotho_ir::ParameterDecl",
            vec![
                field("name", "Name", "parameter name"),
                field("ty", "ParameterType", "value type"),
                field("default", "Option<ParameterValue>", "flatten default"),
            ],
            "(name:\"scale\",ty:I32,default:Some(I32(1)))",
            "(name:\"\",ty:I32,default:None)",
        ),
        structure(
            "klotho_ir::ObjectAnchor",
            vec![
                field("kind", "AnchorKind", "object family"),
                field("name", "Name", "current name"),
                field("anchor", "AnchorId", "frozen identity"),
            ],
            "(kind:Locus,name:\"oak_door\",anchor:\"00000000000000000000000000000000\")",
            "(kind:Locus,name:\"\",anchor:\"00\")",
        ),
        structure(
            "klotho_ir::SourceSpan",
            vec![
                field("module", "AnchorId", "originating module"),
                field("kind", "SpanKind", "item family"),
                field("index", "u32", "index within the family"),
                bounded("start", "u32", "canonical stream start", 0, 4_294_967_295),
                bounded("end", "u32", "canonical stream end", 0, 4_294_967_295),
            ],
            "(module:\"00000000000000000000000000000000\",kind:Seed,index:0,start:0,end:0)",
            "(module:\"00\",kind:Seed,index:0,start:0,end:0)",
        ),
        enumeration(
            "klotho_ir::AnchorKind",
            [
                ("Module", 0, "module identity"),
                ("Locus", 1, "seed locus"),
                ("Law", 2, "law id"),
                ("Affordance", 3, "affordance id"),
                ("Rite", 4, "rite id"),
                ("Beat", 5, "beat id"),
                ("Mind", 6, "mind spec"),
            ]
            .into_iter()
            .map(|(n, d, desc)| variant(n, Some(d), "unit", desc))
            .collect(),
            "Locus",
        ),
        enumeration(
            "klotho_ir::ParameterType",
            {
                ["Name", "I32", "Bool", "Anchor"]
                    .into_iter()
                    .map(|n| variant(n, None, "unit", "module parameter type"))
                    .collect()
            },
            "I32",
        ),
        structure(
            "klotho_ir::StyleIntent",
            vec![
                field("notes", "String", "free-form visual direction"),
                field("palettes", "Vec<Name>", "palette ids"),
                field("kitbash_tags", "Vec<Name>", "required asset tags"),
            ],
            "(notes:\"readable\",palettes:[\"stone\"],kitbash_tags:[])",
            "(notes:\"\",unknown:[] )",
        ),
        structure(
            "klotho_ir::MindSpec",
            vec![
                field("locus", "Name", "seed actor"),
                field("goals", "Vec<Name>", "goal ids"),
                field("templates", "Vec<String>", "dialogue templates"),
            ],
            "(locus:\"smith\",goals:[\"work\"],templates:[])",
            "(locus:\"\",goals:[],templates:[] )",
        ),
        structure(
            "klotho_ir::Law",
            vec![
                field("id", "Name", "law id"),
                field("when", "Pred", "selection predicate"),
                field("body", "LawBody", "admission or continuous body"),
            ],
            "(id:\"alive\",when:SelfIs(Self),body:Pred(must:SelfIs(Self),ought:None))",
            "(id:\"\",when:SelfIs(Self),body:Pred(must:SelfIs(Self),ought:None))",
        ),
        structure(
            "klotho_ir::Cost",
            vec![
                field("res", "Name", "resource id"),
                field("amount", "i32", "amount spent"),
            ],
            "(res:\"stamina\",amount:1)",
            "(res:\"\",amount:1)",
        ),
        structure(
            "klotho_ir::Affordance",
            vec![
                field("id", "Name", "capability id"),
                field("requires", "Vec<Pred>", "eligibility predicates"),
                field("grants", "Vec<Name>", "verb or Rite tags"),
                field("conflicts", "Vec<Name>", "exclusive capabilities"),
            ],
            "(id:\"Portable\",requires:[],grants:[\"Carry\"],conflicts:[])",
            "(id:\"\",requires:[],grants:[],conflicts:[])",
        ),
        structure(
            "klotho_ir::RiteGraph",
            vec![
                field("id", "Name", "Rite id"),
                bounded("cap_steps", "u16", "per-tick instruction cap", 1, 65535),
                bounded("cap_ticks", "u16", "wall-tick cap", 1, 65535),
                field("entry", "u16", "entry pc"),
                field("nodes", "Vec<RiteNode>", "instruction graph"),
            ],
            "(id:\"open\",cap_steps:8,cap_ticks:30,entry:0,nodes:[Op(Halt(Success))])",
            "(id:\"open\",cap_steps:0,cap_ticks:0,entry:0,nodes:[])",
        ),
        structure(
            "klotho_ir::Beat",
            vec![
                field("id", "Name", "Beat id"),
                field("notes", "String", "author notes"),
            ],
            "(id:\"arrival\",notes:\"\")",
            "(id:\"\",notes:\"\")",
        ),
        structure(
            "klotho_ir::Analog",
            vec![
                bounded("phase", "u16", "WAIT phase per-mille", 0, 1000),
                field("stick_x", "i16", "lateral input"),
                field("stick_z", "i16", "forward input"),
                field("look_yaw", "YawMd", "yaw delta"),
                field("look_pitch", "i32", "pitch delta"),
            ],
            "(phase:0,stick_x:0,stick_z:0,look_yaw:0,look_pitch:0)",
            "(phase:1001,stick_x:0,stick_z:0,look_yaw:0,look_pitch:0)",
        ),
        structure(
            "klotho_ir::Agency",
            vec![
                field("claimed", "Vec<Channel>", "claimed player-only channels"),
                field("assist", "AssistLevel", "assist policy"),
            ],
            "(claimed:[],assist:None)",
            "(claimed:[Timing,Timing],assist:None)",
        ),
        enumeration(
            "klotho_ir::CanonDiff",
            canon_diff_variants(),
            "AddBeat((id:\"arrival\",notes:\"\"))",
        ),
        enumeration(
            "klotho_ir::LawBody",
            law_body_variants(),
            "Pred(must:SelfIs(Self),ought:None)",
        ),
        enumeration(
            "klotho_ir::RiteNode",
            vec![
                variant("Op", None, "RiteOp", "implicit pc"),
                variant("Labeled", None, "{pc:u16,op:RiteOp}", "explicit pc"),
            ],
            "Op(Halt(Success))",
        ),
        enumeration("klotho_ir::RiteOp", rite_ops(), "Halt(Success)"),
        enumeration("klotho_ir::Pred", predicates(), "SelfIs(Self)"),
        enumeration(
            "klotho_ir::SeedFact",
            seed_variants(),
            "Locus(name:\"hero\",kind:Actor)",
        ),
        enumeration(
            "klotho_ir::Slot",
            vec![
                variant("Self", None, "unit", "acting locus"),
                variant("Target", None, "unit", "intent target"),
                variant("Other", None, "unit", "quantifier binding"),
                variant("Name", None, "Name", "cook-time pin"),
            ],
            "Self",
        ),
        enumeration(
            "klotho_ir::IntentTarget",
            vec![
                variant("None", None, "unit", "no target"),
                variant("Sigil", None, "Sigil", "packed runtime target"),
                variant("Name", None, "Name", "authoring target"),
            ],
            "None",
        ),
        enumeration(
            "klotho_ir::Cmp",
            vec![
                variant("Lt", None, "unit", "less"),
                variant("Le", None, "unit", "less or equal"),
                variant("Eq", None, "unit", "equal"),
                variant("Ge", None, "unit", "greater or equal"),
                variant("Gt", None, "unit", "greater"),
            ],
            "Eq",
        ),
        enumeration("klotho_ir::SourceKind", source_variants(), "Player"),
        enumeration("klotho_ir::Channel", channel_variants(), "Timing"),
        enumeration(
            "klotho_ir::AssistLevel",
            vec![variant("None", Some(0), "unit", "no assist")],
            "None",
        ),
        enumeration("klotho_core::LocusKind", locus_variants(), "Actor"),
        enumeration("klotho_core::SimLod", sim_lod_variants(), "Full"),
        structure(
            "klotho_core::IVec3",
            vec![
                field("x", "i32", "millimetres"),
                field("y", "i32", "millimetres"),
                field("z", "i32", "millimetres"),
            ],
            "(x:0,y:0,z:0)",
            "(x:\"zero\",y:0,z:0)",
        ),
        structure(
            "klotho_core::PoseMm",
            vec![
                field("x", "Mm", "X in millimetres"),
                field("y", "Mm", "Y in millimetres"),
                field("z", "Mm", "Z in millimetres"),
                field("yaw", "YawMd", "yaw in millidegrees"),
                field("pitch", "YawMd", "pitch in millidegrees"),
                field("roll", "YawMd", "roll in millidegrees"),
            ],
            "(x:0,y:0,z:0,yaw:0,pitch:0,roll:0)",
            "(x:\"zero\",y:0,z:0,yaw:0,pitch:0,roll:0)",
        ),
        structure(
            "klotho_ir::Diagnostic",
            vec![
                field("code", "DiagnosticCode", "stable namespaced code"),
                field("class", "FailureClass", "fault corpus class"),
                field("severity", "Severity", "error, warning, or advice"),
                field("message", "String", "concise human line"),
                field("primary", "AnchorId", "primary semantic identity"),
                field("related", "Vec<AnchorId>", "additional blamed objects"),
                field(
                    "witness",
                    "Option<Counterexample>",
                    "minimal counterexample",
                ),
                field(
                    "legal_repairs",
                    "Vec<RepairShape>",
                    "legal repair operations",
                ),
                field("cost", "Option<EstimatedCost>", "repair cost attribution"),
            ],
            "(code:\"KAI-DIAG-CONTRADICTION\",class:contradiction,severity:Error,message:\"Contradiction(a|b)\",primary:\"00000000000000000000000000000000\",related:[],witness:None,legal_repairs:[],cost:None)",
            "(code:\"\",class:unknown,severity:Error,message:\"\",primary:\"00\",related:[],witness:None,legal_repairs:[],cost:None)",
        ),
        structure(
            "klotho_ir::RepairShape",
            vec![
                field("op", "String", "catalog operation id"),
                field("note", "String", "legal repair note"),
            ],
            "(op:\"author.remove@1\",note:\"remove one scoped fact\")",
            "(op:\"\",note:\"\")",
        ),
        structure(
            "klotho_ir::EstimatedCost",
            vec![
                bounded("units", "u16", "catalog cost_units", 0, 65535),
                field("stage", "String", "validate, cook, package, prove, eval"),
            ],
            "(units:3,stage:\"cook\")",
            "(units:\"x\",stage:\"\")",
        ),
        enumeration(
            "klotho_ir::FailureClass",
            [
                ("Contradiction", "unsatisfiable Laws"),
                ("Cfg", "Rite control-flow"),
                ("Cap", "frozen pred/rite cap"),
                ("Agency", "player-only Agency"),
                ("Provenance", "license/CAS"),
                ("Package", "ship allowlist/warp"),
                ("Journey", "unreachable assertion"),
                ("Budget", "budget miss"),
                ("Reproducibility", "hash drift"),
                ("Schema", "parse/module structure"),
            ]
            .into_iter()
            .map(|(n, desc)| variant(n, None, "unit", desc))
            .collect(),
            "Contradiction",
        ),
        enumeration(
            "klotho_ir::Severity",
            [
                ("Error", "gate failure"),
                ("Warning", "non-blocking"),
                ("Advice", "advisory critic"),
            ]
            .into_iter()
            .map(|(n, desc)| variant(n, None, "unit", desc))
            .collect(),
            "Error",
        ),
    ]
}

fn canon_diff_variants() -> Vec<VariantSchema> {
    [
        ("AddLaw", "Law"),
        ("RetractLaw", "{id:Name,reason:String}"),
        ("AddAffordance", "Affordance"),
        ("AddRite", "RiteGraph"),
        ("RetractRite", "{id:Name,reason:String}"),
        ("AddBeat", "Beat"),
    ]
    .into_iter()
    .map(|(n, p)| variant(n, None, p, "Canon patch"))
    .collect()
}

fn law_body_variants() -> Vec<VariantSchema> {
    [
        ("Pred", "{must:Pred,ought:Option<Cost>}"),
        ("Ramp", "{res:Name,per_tick:i32,quantum:i32,cap:i32}"),
        (
            "Spread",
            "{res:Name,per_tick:i32,near:Mm,cap_global:u16,ignite_at:i32}",
        ),
        ("Conserve", "{res:Name,over:Rel}"),
        ("Cap", "{mark:Pred,n:u16,require_rel:Option<(Rel,Slot)>}"),
    ]
    .into_iter()
    .map(|(n, p)| variant(n, None, p, "Law body"))
    .collect()
}

fn seed_variants() -> Vec<VariantSchema> {
    [
        ("Locus", "{name:Name,kind:LocusKind}"),
        ("Rel", "{a:Name,rel:Rel,b:Name}"),
        ("Qty", "{of:Name,res:Name,value:i32}"),
        ("Pose", "{of:Name,pose:PoseMm}"),
    ]
    .into_iter()
    .map(|(n, p)| variant(n, None, p, "seed Trace fact"))
    .collect()
}

fn predicates() -> Vec<VariantSchema> {
    [
        ("Affordance", "(Slot,Name)"),
        ("Rel", "(Slot,Rel,Slot)"),
        ("Qty", "(Slot,Name,Cmp,i32)"),
        ("EqVerb", "Verb"),
        ("RiteActive", "Name"),
        ("AabbNear", "(Slot,Slot,Mm)"),
        ("InWindow", "(Name,Channel)"),
        ("Knows", "(Slot,Name)"),
        ("SourceIs", "SourceKind"),
        ("AgencyClaimed", "Channel"),
        ("Burning", "Slot"),
        ("OpaqueClosed", "Slot"),
        ("SweptHitsOpaqueClosed", "unit"),
        ("IslandAwake", "Slot"),
        ("SelfIs", "Slot"),
        ("TargetIs", "Slot"),
        ("OtherIs", "Slot"),
        ("RayHits", "{from:Slot,dir:IVec3,max:Mm,mask:u8}"),
        ("SimLodIs", "(Slot,SimLod)"),
        ("InPlace", "(Slot,Slot)"),
        ("And", "(Pred,Pred)"),
        ("Or", "(Pred,Pred)"),
        ("Not", "Pred"),
        ("ExistsRelated", "{of:Slot,rel:Rel,pred:Pred}"),
        ("CountRelated", "{of:Slot,rel:Rel,pred:Pred,cmp:Cmp,n:i32}"),
    ]
    .into_iter()
    .map(|(n, p)| variant(n, None, p, "authoring predicate"))
    .collect()
}

fn rite_ops() -> Vec<VariantSchema> {
    [
        ("Halt", "Status"),
        ("Guard", "(Pred,u16)"),
        ("Spend", "(Name,i32,u16)"),
        ("Wait", "(u16,Option<Channel>)"),
        ("Emit", "Name"),
        ("Branch", "(Pred,u16,u16)"),
        ("Bind", "BindSrc"),
        ("Setq", "(Slot,Name,i32)"),
        ("RelAdd", "(Slot,Rel,Slot)"),
        ("RelDel", "(Slot,Rel,Slot)"),
        ("Awake", "Slot"),
        ("Complete", "Status"),
        ("Spawn", "Name"),
        ("PhysReq", "{lin:IVec3,ang:IVec3}"),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (n, p))| variant(n, Some(i as u16), p, "Rite ISA operation"))
    .collect()
}

fn source_variants() -> Vec<VariantSchema> {
    [
        "Player",
        "Mind",
        "Space",
        "Motion",
        "Infer",
        "Phys",
        "Residency",
    ]
    .into_iter()
    .enumerate()
    .map(|(i, n)| variant(n, Some(i as u16), "unit", "proposal source"))
    .collect()
}

fn channel_variants() -> Vec<VariantSchema> {
    ["Timing", "Aim", "ResourceSpend", "DialogueChoice"]
        .into_iter()
        .enumerate()
        .map(|(i, n)| variant(n, Some((i + 1) as u16), "unit", "player-only skill channel"))
        .collect()
}

fn locus_variants() -> Vec<VariantSchema> {
    (1u8..)
        .map_while(LocusKind::from_u8)
        .map(|kind| {
            variant(
                &format!("{kind:?}"),
                Some(u16::from(kind.as_u8())),
                "unit",
                "locus kind",
            )
        })
        .collect()
}

fn sim_lod_variants() -> Vec<VariantSchema> {
    (0u8..)
        .map_while(SimLod::from_u8)
        .map(|lod| {
            variant(
                &format!("{lod:?}"),
                Some(u16::from(lod.as_u8())),
                "unit",
                "simulation rate class",
            )
        })
        .collect()
}

fn verbs() -> Vec<TagSchema> {
    (0u8..)
        .map_while(Verb::from_u8)
        .map(|verb| TagSchema {
            name: format!("{verb:?}"),
            discriminant: u16::from(verb.as_u8()),
        })
        .collect()
}

fn rels() -> Vec<TagSchema> {
    (0u8..)
        .map_while(Rel::from_u8)
        .map(|rel| TagSchema {
            name: rel.as_str().to_owned(),
            discriminant: u16::from(rel.as_u8()),
        })
        .collect()
}

fn diagnostics() -> Vec<DiagnosticSchema> {
    klotho_ir::diagnostic_catalog()
        .iter()
        .map(|entry| DiagnosticSchema {
            code: entry.code.to_owned(),
            source: entry.source.to_owned(),
            class: entry.class.as_str().to_owned(),
            retryable: entry.retryable,
            legal_repairs: entry
                .legal_repairs
                .iter()
                .map(|note| (*note).to_owned())
                .collect(),
        })
        .collect()
}

fn budget(name: &str, value: Budget) -> BudgetSchema {
    BudgetSchema {
        name: name.to_owned(),
        us_sim: value.us_sim,
        pred_ops: value.pred_ops,
        rite_steps: value.rite_steps,
        eval_slo_ticks: value.eval_slo_ticks,
        rewind_ticks: value.rewind_ticks,
    }
}

fn budgets() -> Vec<BudgetSchema> {
    vec![
        budget("hearth", Budget::HEARTH),
        budget("aaa_adventure", Budget::AAA_ADVENTURE),
        budget("aaa_shooter", Budget::AAA_SHOOTER),
    ]
}

fn operations() -> Vec<OperationSchema> {
    [
        ("locus.add@1", "SeedFact::Locus", &["seed"] as &[_], 1),
        ("locus.remove@1", "Name", &["seed", "canon_diffs"], 3),
        (
            "locus.rename@1",
            "{from:Name,to:Name}",
            &["seed", "canon_diffs", "minds"],
            2,
        ),
        ("canon.diff.add@1", "CanonDiff", &["canon_diffs"], 2),
        ("seed.fact.add@1", "SeedFact", &["seed"], 1),
        ("mind.set@1", "MindSpec", &["minds"], 1),
        ("style.set@1", "StyleIntent", &["style"], 1),
        (
            "reference.replace@1",
            "{old:ProvenanceId,new:ProvenanceId}",
            &["provenance"],
            2,
        ),
        ("module.add@1", "IntentModule", &["modules", "lock"], 3),
        ("module.import@1", "ModuleImport", &["imports", "lock"], 2),
        (
            "anchor.rename@1",
            "{target:AnchorId,to:Name}",
            &["body", "aliases"],
            2,
        ),
        (
            "object.tombstone@1",
            "Tombstone",
            &["tombstones", "body"],
            3,
        ),
        ("author.instantiate@1", "PatternInstance", &["body"], 4),
        (
            "author.set_argument@1",
            "{instance:AnchorId,key:Name,value:PatternArg}",
            &["parameters"],
            1,
        ),
        (
            "author.add_locus@1",
            "{module:AnchorId,anchor:AnchorId,name:Name,kind:LocusKind}",
            &["seed", "object_anchors"],
            1,
        ),
        (
            "author.add_fact@1",
            "{module:AnchorId,fact:AnchoredSeedFact}",
            &["seed"],
            1,
        ),
        (
            "author.bind_asset@1",
            "{locus:AnchorId,request:AssetRequestId}",
            &["bindings"],
            1,
        ),
        ("author.add_journey@1", "JourneySpec", &["journeys"], 3),
        (
            "author.add_reference@1",
            "{target:AnchorId,reference:ReferenceId}",
            &["references"],
            1,
        ),
        (
            "author.remove@1",
            "{target:AnchorId,reason:String}",
            &["tombstones", "body"],
            3,
        ),
        ("author.transaction.submit@1", "ChangeId", &["review"], 1),
    ]
    .into_iter()
    .map(|(id, input, writes, cost_units)| OperationSchema {
        id: id.to_owned(),
        input: input.to_owned(),
        writes: writes.iter().map(|s| (*s).to_owned()).collect(),
        cost_units,
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use klotho_core::{Mm, PoseMm, YawMd};
    use klotho_ir::{
        Agency, Analog, AssistLevel, Beat, CanonDiff, Channel, Cost, IntentDoc, MindSpec, Name,
        Pred, SeedFact, Slot, StyleIntent,
    };
    use klotho_prove::ProvenanceId;
    use serde::de::DeserializeOwned;

    fn round_trip<T>(value: &T)
    where
        T: Serialize + DeserializeOwned + PartialEq + core::fmt::Debug,
    {
        let ron = ron::ser::to_string(value).expect("serialize public authoring type");
        let decoded: T = ron::from_str(&ron).expect("deserialize public authoring type");
        assert_eq!(&decoded, value);
    }

    #[test]
    fn catalog_is_sorted_complete_and_self_hashed() {
        let catalog = generate(&Canon::default());
        assert_eq!(catalog.version, SCHEMA_VERSION);
        assert_ne!(catalog.toolchain_hash, Hash::ZERO);
        assert_eq!(catalog.verbs.len(), 13);
        assert_eq!(catalog.rels.len(), 13);
        assert_eq!(catalog.predicates.len(), 25);
        assert_eq!(catalog.rite_ops.len(), 14);
        assert!(
            catalog
                .kinds
                .iter()
                .all(|kind| !kind.positive_example.is_empty() && !kind.negative_example.is_empty())
        );
    }

    #[test]
    fn every_public_authoring_root_round_trips() {
        let name = Name::from("hero");
        round_trip(&StyleIntent {
            notes: "readable".to_owned(),
            palettes: vec![Name::from("stone")],
            kitbash_tags: Vec::new(),
        });
        round_trip(&Agency {
            claimed: vec![Channel::Timing],
            assist: AssistLevel::None,
        });
        round_trip(&Analog {
            phase: 500,
            stick_x: -1,
            stick_z: 2,
            look_yaw: YawMd(3),
            look_pitch: 4,
        });
        round_trip(&Cost {
            res: Name::from("stamina"),
            amount: 1,
        });
        round_trip(&MindSpec {
            locus: name.clone(),
            goals: vec![Name::from("survive")],
            templates: vec!["Wait.".to_owned()],
        });
        round_trip(&Pred::AabbNear(Slot::This, Slot::Target, Mm(10)));
        round_trip(&SeedFact::Pose {
            of: name.clone(),
            pose: PoseMm {
                x: Mm(1),
                y: Mm(2),
                z: Mm(3),
                yaw: YawMd(4),
                pitch: YawMd(5),
                roll: YawMd(6),
            },
        });
        round_trip(&CanonDiff::AddBeat(Beat {
            id: Name::from("arrival"),
            notes: String::new(),
        }));
        round_trip(&IntentDoc {
            style: StyleIntent::default(),
            canon_diffs: Vec::new(),
            seed: vec![SeedFact::Locus {
                name: name.clone(),
                kind: LocusKind::Actor,
            }],
            minds: Vec::new(),
            provenance: ProvenanceId(Hash::ZERO),
        });
        let bundle = klotho_ir::migrate_doc(
            Name::from("hearth"),
            Name::from("main"),
            IntentDoc {
                style: StyleIntent::default(),
                canon_diffs: Vec::new(),
                seed: vec![SeedFact::Locus {
                    name,
                    kind: LocusKind::Actor,
                }],
                minds: Vec::new(),
                provenance: ProvenanceId(Hash::ZERO),
            },
        )
        .unwrap();
        round_trip(&bundle.project);
        round_trip(&bundle.modules[0]);
        round_trip(&bundle.modules[0].anchor);
    }

    #[test]
    fn stable_discriminants_are_unique_and_dense() {
        for (index, tag) in verbs().iter().enumerate() {
            assert_eq!(usize::from(tag.discriminant), index);
        }
        for (index, tag) in rels().iter().enumerate() {
            assert_eq!(usize::from(tag.discriminant), index);
        }
        for (index, item) in locus_variants().iter().enumerate() {
            assert_eq!(usize::from(item.discriminant.expect("tag")), index + 1);
        }
    }

    #[test]
    fn catalog_lists_every_seeded_diagnostic_code() {
        let catalog = generate(&Canon::default());
        let codes: Vec<_> = catalog
            .diagnostics
            .iter()
            .map(|d| d.code.as_str())
            .collect();
        for needed in [
            "KAI-DIAG-CONTRADICTION",
            "KAI-DIAG-CFG-TARGET",
            "KAI-DIAG-CAP",
            "KAI-DIAG-AGENCY",
            "KAI-DIAG-PROVENANCE",
            "KAI-DIAG-PACKAGE",
            "KAI-DIAG-JOURNEY",
            "KAI-DIAG-BUDGET",
            "KAI-DIAG-HASH-DRIFT",
        ] {
            assert!(codes.contains(&needed), "missing {needed}");
        }
        assert!(
            catalog
                .diagnostics
                .iter()
                .filter(|d| d.code.starts_with("KAI-DIAG-"))
                .all(|d| !d.legal_repairs.is_empty())
        );
    }

    #[test]
    fn golden_matches_generated_catalog() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("goldens/schema-v1.json");
        check_golden(&generate(&Canon::default()), &path).expect("schema golden");
    }
}
