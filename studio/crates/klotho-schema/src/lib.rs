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
    /// Pattern versions from `klotho-pattern`.
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

/// Registered reusable pattern.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatternSchema {
    /// Stable pattern id.
    pub id: String,
    /// Append-only pattern version.
    pub version: u32,
    /// Standard-library family.
    pub family: String,
    /// Parameter names.
    pub parameters: Vec<String>,
    /// `param:cap` requires.
    pub requires: Vec<String>,
    /// `param:cap` grants.
    pub grants: Vec<String>,
    /// `param:cap` conflicts.
    pub conflicts: Vec<String>,
    /// Predicate-node budget.
    pub predicates: u32,
    /// Rite-instruction budget.
    pub rite_steps: u32,
    /// Per-tick writer budget.
    pub per_tick: u32,
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
        patterns: pattern_schemas(),
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
                field(
                    "patterns",
                    "Vec<PatternInstance>",
                    "unexpanded pattern instances",
                ),
                field("body", "IntentDoc", "ordinary Intent body"),
            ],
            "(anchor:\"00000000000000000000000000000000\",id:\"main\",version:1,imports:[],exports:[],parameters:[],aliases:[],tombstones:[],object_anchors:[],patterns:[],body:(style:(notes:\"\",palettes:[],kitbash_tags:[]),canon_diffs:[],seed:[],minds:[],provenance:\"0000000000000000000000000000000000000000000000000000000000000000\"))",
            "(anchor:\"00\",id:\"\",version:0,imports:[],exports:[],parameters:[],aliases:[],tombstones:[],object_anchors:[],patterns:[],body:())",
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
            "klotho_ir::PatternArg",
            vec![
                field("key", "Name", "parameter name"),
                field("value", "ParameterValue", "bound value"),
            ],
            "(key:\"passage\",value:Name(\"oak_door\"))",
            "(key:\"\",value:Name(\"\"))",
        ),
        structure(
            "klotho_ir::PatternInstance",
            vec![
                field("anchor", "AnchorId", "frozen instance identity"),
                field("module", "AnchorId", "owning module"),
                field("instance", "Name", "authoring name"),
                field("pattern", "Name", "standard-library id"),
                field("version", "u32", "pattern version"),
                field("args", "Vec<PatternArg>", "bound arguments"),
            ],
            "(anchor:\"00000000000000000000000000000000\",module:\"00000000000000000000000000000000\",instance:\"gate\",pattern:\"traversal.door_key\",version:1,args:[])",
            "(anchor:\"00\",module:\"00\",instance:\"\",pattern:\"\",version:0,args:[])",
        ),
        structure(
            "klotho_pattern::WorldPlan",
            vec![
                field("anchor", "AnchorId", "immutable plan identity"),
                field("id", "Name", "authoring id"),
                field("places", "Vec<PlacePlan>", "Places; validate sorts by name"),
                field("edges", "Vec<TraversalEdge>", "traversal graph"),
                field("critical_path", "Vec<Name>", "ordered critical journey"),
                field(
                    "protected",
                    "Vec<AnchorId>",
                    "anchors regeneration must keep",
                ),
            ],
            "(anchor:\"00000000000000000000000000000000\",id:\"greybox\",places:[],edges:[],critical_path:[\"hub\"],protected:[])",
            "(anchor:\"00\",id:\"\",places:[],edges:[],critical_path:[],protected:[])",
        ),
        structure(
            "klotho_pattern::PlacePlan",
            vec![
                field("name", "Name", "authoring name"),
                field("role", "PlaceRole", "route role"),
                field("envelope", "AabbMm", "streaming envelope"),
                field("budgets", "PlaceBudgets", "multidomain caps"),
                field(
                    "protected",
                    "Vec<AnchorId>",
                    "Place-local protected anchors",
                ),
                field("zones", "Vec<Name>", "dressing zones"),
            ],
            "(name:\"hub\",role:hub,envelope:(min:(x:0,y:0,z:0),max:(x:1,y:1,z:1)),budgets:(density:8,visibility:4,streaming_bytes:1024,nav_cells:8,phys_bodies:8,audio_voices:2,gpu_instances:8),protected:[],zones:[\"dress\"])",
            "(name:\"\",role:hub,envelope:(min:(x:1,y:0,z:0),max:(x:0,y:0,z:0)),budgets:(density:0,visibility:0,streaming_bytes:0,nav_cells:0,phys_bodies:0,audio_voices:0,gpu_instances:0),protected:[],zones:[])",
        ),
        structure(
            "klotho_pattern::PlaceBudgets",
            vec![
                field("density", "u32", "max dressing instances"),
                field("visibility", "u32", "max unique bindings"),
                field("streaming_bytes", "u64", "max unique dressing bytes"),
                field("nav_cells", "u32", "max nav cost"),
                field("phys_bodies", "u32", "max Phys cost"),
                field("audio_voices", "u32", "max audio voices"),
                field("gpu_instances", "u32", "max GPU instances"),
            ],
            "(density:64,visibility:16,streaming_bytes:65536,nav_cells:64,phys_bodies:64,audio_voices:8,gpu_instances:64)",
            "(density:-1,visibility:0,streaming_bytes:0,nav_cells:0,phys_bodies:0,audio_voices:0,gpu_instances:0)",
        ),
        structure(
            "klotho_pattern::TraversalEdge",
            vec![
                field("from", "Name", "origin Place"),
                field("to", "Name", "destination Place"),
                field("bidirectional", "bool", "implied reverse edge"),
                field("kind", "EdgeKind", "critical, optional, or shortcut"),
            ],
            "(from:\"hub\",to:\"combat\",bidirectional:false,kind:critical)",
            "(from:\"\",to:\"\",bidirectional:false,kind:critical)",
        ),
        structure(
            "klotho_pattern::DressingInstance",
            vec![
                field("place", "Name", "owning Place"),
                field("zone", "Name", "owning zone"),
                field("mesh", "BlobId", "shared mesh"),
                field("material", "BlobId", "shared material"),
                field("clip", "Option<BlobId>", "optional clip"),
                field("variant", "u16", "variant index"),
                field("pose", "IVec3", "translation mm"),
                field("yaw", "YawMd", "yaw millidegrees"),
                field("scale_permille", "u16", "uniform scale, 1000 = 1"),
                field("blob_bytes", "u32", "unique source bytes for streaming"),
                field("nav_cost", "u32", "nav-cell cost"),
                field("phys_cost", "u32", "Phys-body cost"),
                field("audio_cost", "u32", "audio-voice cost"),
                field("protected", "bool", "solver may not drop"),
            ],
            "(place:\"hub\",zone:\"dress\",mesh:\"0000000000000000000000000000000000000000000000000000000000000000\",material:\"0000000000000000000000000000000000000000000000000000000000000000\",clip:None,variant:0,pose:(x:0,y:0,z:0),yaw:0,scale_permille:1000,blob_bytes:256,nav_cost:1,phys_cost:1,audio_cost:0,protected:true)",
            "(place:\"\",zone:\"\",mesh:\"00\",material:\"00\",clip:None,variant:0,pose:(x:0,y:0,z:0),yaw:0,scale_permille:0,blob_bytes:0,nav_cost:0,phys_cost:0,audio_cost:0,protected:false)",
        ),
        enumeration(
            "klotho_pattern::PlaceRole",
            [
                ("hub", 0, "safe hub"),
                ("combat_pocket", 1, "combat pocket"),
                ("traversal", 2, "traversal beat"),
                ("conversation", 3, "conversation beat"),
                ("cinematic", 4, "cinematic beat"),
                ("checkpoint", 5, "checkpoint / rest"),
                ("shortcut", 6, "return shortcut"),
                ("optional", 7, "optional objective"),
            ]
            .into_iter()
            .map(|(n, d, desc)| variant(n, Some(d), "unit", desc))
            .collect(),
            "hub",
        ),
        enumeration(
            "klotho_pattern::EdgeKind",
            [
                ("critical", 0, "required critical-path connection"),
                ("optional", 1, "optional branch"),
                ("shortcut", 2, "return / skip shortcut"),
            ]
            .into_iter()
            .map(|(n, d, desc)| variant(n, Some(d), "unit", desc))
            .collect(),
            "critical",
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
                ("Pattern", 7, "pattern instance"),
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
            "klotho_ir::MindProgram",
            vec![
                field("beat", "Option<Name>", "encounter Beat identity"),
                field("facts", "Vec<MindFact>", "bounded visible facts; cap 64"),
                field(
                    "operators",
                    "Vec<MindOperator>",
                    "compiled GOAP operators; cap 32",
                ),
                field("goals", "Vec<MindGoal>", "integer utility goals; cap 4"),
                field(
                    "far",
                    "Vec<FarRule>",
                    "direct Far policy table; cap 16 inputs",
                ),
            ],
            "(beat:None,facts:[],operators:[],goals:[],far:[])",
            "(beat:None,facts:[],operators:[],goals:[],far:[],unknown:1)",
        ),
        structure(
            "klotho_ir::MindFact",
            vec![
                field("id", "Name", "program-local fact id"),
                field("query", "MindQuery", "Projection-derived input"),
                field("far_safe", "bool", "Far refinement permission"),
            ],
            "(id:\"ready\",query:Always,far_safe:true)",
            "(id:\"\",query:Always,far_safe:true)",
        ),
        enumeration(
            "klotho_ir::MindRef",
            vec![
                variant("This", None, "unit", "program owner"),
                variant("Pin", None, "Name", "immutable seed pin"),
                variant("Related", None, "Rel", "first relation target"),
            ],
            "This",
        ),
        enumeration(
            "klotho_ir::MindQuery",
            vec![
                variant("Never", None, "unit", "constant false"),
                variant("Always", None, "unit", "constant true"),
                variant(
                    "Related",
                    None,
                    "{a:MindRef,rel:Rel,b:MindRef}",
                    "relation membership",
                ),
                variant(
                    "QtyAtLeast",
                    None,
                    "{of:MindRef,res:Name,min:i32}",
                    "quantity threshold",
                ),
                variant(
                    "AnyQtyAtLeast",
                    None,
                    "{res:Name,min:i32}",
                    "global quantity threshold",
                ),
                variant(
                    "Near",
                    None,
                    "{a:MindRef,b:MindRef,within:Mm}",
                    "integer XZ distance",
                ),
                variant(
                    "TickModulo",
                    None,
                    "{period:u16,phase:u16}",
                    "deterministic Beat clock",
                ),
            ],
            "Always",
        ),
        enumeration(
            "klotho_ir::MindTarget",
            vec![
                variant("None", None, "unit", "no target"),
                variant("Ref", None, "MindRef", "resolved locus target"),
            ],
            "None",
        ),
        structure(
            "klotho_ir::MindOperator",
            vec![
                field("id", "Name", "operator id"),
                field("requires", "Vec<Name>", "required fact ids"),
                field("sets", "Vec<Name>", "facts made true"),
                field("clears", "Vec<Name>", "facts made false"),
                bounded("cost", "u16", "non-zero integer cost", 1, 65535),
                field("verb", "Verb", "emitted action"),
                field("target", "MindTarget", "action target"),
            ],
            "(id:\"act\",requires:[],sets:[\"done\"],clears:[],cost:1,verb:Investigate,target:None)",
            "(id:\"act\",requires:[],sets:[],clears:[],cost:0,verb:Time,target:None)",
        ),
        structure(
            "klotho_ir::MindGoal",
            vec![
                field("id", "Name", "goal id"),
                field("desired", "Vec<Name>", "desired fact ids"),
                bounded("utility", "u16", "integer rank", 0, 65535),
            ],
            "(id:\"work\",desired:[\"done\"],utility:10)",
            "(id:\"work\",desired:[],utility:10)",
        ),
        structure(
            "klotho_ir::FarRule",
            vec![
                field("requires", "Vec<Name>", "FarSafe table inputs"),
                field("effects", "Vec<Name>", "FarSafe affected facts"),
                field("verb", "Verb", "emitted action"),
                field("target", "MindTarget", "action target"),
                bounded("utility", "u16", "integer rank", 0, 65535),
            ],
            "(requires:[\"ready\"],effects:[\"wandered\"],verb:Move,target:None,utility:1)",
            "(requires:[\"protected\"],effects:[],verb:Move,target:None,utility:1)",
        ),
        structure(
            "klotho_ir::MindSpec",
            vec![
                field("locus", "Name", "seed actor"),
                field("program", "MindProgram", "compiled Mind tables"),
                field("templates", "Vec<String>", "dialogue templates"),
            ],
            "(locus:\"smith\",program:(beat:None,facts:[],operators:[],goals:[],far:[]),templates:[])",
            "(locus:\"\",program:(beat:None,facts:[],operators:[],goals:[],far:[]),templates:[])",
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
        structure(
            "klotho_ir::TickWindow",
            vec![
                bounded("start", "u8", "inclusive start tick", 0, 32),
                bounded("end", "u8", "inclusive end tick", 0, 32),
                field("verb", "Verb", "verb this window admits"),
            ],
            "(start:0,end:3,verb:Drop)",
            "(start:4,end:1,verb:Use)",
        ),
        structure(
            "klotho_ir::CurveKnot",
            vec![
                bounded("x", "u16", "stick magnitude per-mille", 0, 1000),
                field("y", "i32", "16.16 scale factor"),
            ],
            "(x:0,y:0)",
            "(x:1001,y:0)",
        ),
        structure(
            "klotho_ir::QuantizedCurve",
            vec![field(
                "knots",
                "Vec<CurveKnot>",
                "strictly increasing 0..=1000",
            )],
            "(knots:[(x:0,y:0),(x:1000,y:65536)])",
            "(knots:[(x:100,y:0)])",
        ),
        structure(
            "klotho_ir::CameraResponse",
            vec![
                field("smoothing_ticks", "u8", "presentation lerp horizon"),
                bounded("follow_stiffness", "u16", "per-mille follow", 0, 1000),
                field("shake_amp_mm", "u16", "authored shake millimetres"),
                field("shake_cap_mm", "u16", "accessibility shake cap"),
                field("accel_cap_md", "u32", "look accel cap millidegrees/tick^2"),
                field("hull_radius_mm", "u16", "collision-free camera hull"),
            ],
            "(smoothing_ticks:2,follow_stiffness:500,shake_amp_mm:8,shake_cap_mm:16,accel_cap_md:12000,hull_radius_mm:250)",
            "(smoothing_ticks:2,follow_stiffness:1001,shake_amp_mm:8,shake_cap_mm:16,accel_cap_md:12000,hull_radius_mm:250)",
        ),
        structure(
            "klotho_ir::AimAssistContract",
            vec![
                bounded("magnet_permille", "u16", "yaw-error pull", 0, 1000),
                field("cone_md", "u32", "magnet cone millidegrees"),
                field("max_correction_md", "u32", "per-tick cap millidegrees"),
            ],
            "(magnet_permille:250,cone_md:8000,max_correction_md:2000)",
            "(magnet_permille:1001,cone_md:0,max_correction_md:0)",
        ),
        structure(
            "klotho_ir::ImpactPresentation",
            vec![
                field(
                    "hit_stop_present_ticks",
                    "u8",
                    "Manifest freeze; tick still advances",
                ),
                field("recovery_wait_ticks", "u8", "authoritative Rite WAIT"),
                field("shake_amp_mm", "u16", "impact shake millimetres"),
            ],
            "(hit_stop_present_ticks:2,recovery_wait_ticks:4,shake_amp_mm:6)",
            "(hit_stop_present_ticks:2,recovery_wait_ticks:4,shake_amp_mm:6,pause_tick:true)",
        ),
        structure(
            "klotho_ir::FeelAccessibility",
            vec![
                field("reduce_shake", "bool", "zero presented shake"),
                field("reduce_haptics", "bool", "suppress haptic cue"),
                field("hold_to_toggle", "bool", "hold becomes toggle"),
                field("aim_assist_required", "bool", "require AimAssistContract"),
            ],
            "(reduce_shake:false,reduce_haptics:false,hold_to_toggle:false,aim_assist_required:false)",
            "(reduce_shake:false)",
        ),
        structure(
            "klotho_ir::FeelContract",
            vec![
                field("action", "Name", "tuned action"),
                bounded("input_buffer_ticks", "u8", "pending press horizon", 0, 8),
                bounded("coyote_ticks", "u8", "post-support window", 0, 8),
                field("cancel_windows", "Vec<TickWindow>", "cancel windows"),
                field("combo_windows", "Vec<TickWindow>", "combo windows"),
                field("accel_curve", "QuantizedCurve", "stick acceleration"),
                field("decel_curve", "QuantizedCurve", "stick release"),
                field("camera", "CameraResponse", "follow and shake"),
                field("aim_assist", "Option<AimAssistContract>", "analog magnet"),
                field("impact", "ImpactPresentation", "hit-stop vs WAIT recovery"),
                field("haptics", "Name", "haptic pattern; empty is invalid"),
                field("accessibility", "FeelAccessibility", "same-action access"),
            ],
            "(action:\"use\",input_buffer_ticks:2,coyote_ticks:2,cancel_windows:[(start:0,end:3,verb:Drop)],combo_windows:[(start:4,end:8,verb:Use)],accel_curve:(knots:[(x:0,y:0),(x:1000,y:65536)]),decel_curve:(knots:[(x:0,y:0),(x:1000,y:65536)]),camera:(smoothing_ticks:2,follow_stiffness:500,shake_amp_mm:8,shake_cap_mm:16,accel_cap_md:12000,hull_radius_mm:250),aim_assist:None,impact:(hit_stop_present_ticks:2,recovery_wait_ticks:4,shake_amp_mm:6),haptics:\"hit\",accessibility:(reduce_shake:false,reduce_haptics:false,hold_to_toggle:false,aim_assist_required:false))",
            "(action:\"use\",input_buffer_ticks:9,coyote_ticks:2,cancel_windows:[],combo_windows:[],accel_curve:(knots:[]),decel_curve:(knots:[]),camera:(smoothing_ticks:0,follow_stiffness:0,shake_amp_mm:0,shake_cap_mm:0,accel_cap_md:0,hull_radius_mm:0),aim_assist:None,impact:(hit_stop_present_ticks:0,recovery_wait_ticks:0,shake_amp_mm:0),haptics:\"hit\",accessibility:(reduce_shake:false,reduce_haptics:false,hold_to_toggle:false,aim_assist_required:false))",
        ),
        structure(
            "klotho_ir::A11yProfile",
            vec![
                field("remap", "bool", "full action remapping required"),
                field("hold_to_toggle", "bool", "hold becomes toggle"),
                field("subtitles", "bool", "dialogue subtitle band"),
                field("closed_captions", "bool", "SDH / closed captions"),
                bounded(
                    "text_scale_milli",
                    "u16",
                    "UI scale thousandths",
                    750,
                    2_000,
                ),
                field("contrast", "ContrastMode", "default or high"),
                field("reduce_motion", "bool", "suppress non-essential motion"),
                field("reduce_shake", "bool", "zero presented camera shake"),
                field("screen_reader", "bool", "menu screen-reader metadata"),
            ],
            "(remap:true,hold_to_toggle:false,subtitles:true,closed_captions:false,text_scale_milli:1000,contrast:default,reduce_motion:false,reduce_shake:false,screen_reader:false)",
            "(remap:true,hold_to_toggle:false,subtitles:true,closed_captions:false,text_scale_milli:500,contrast:default,reduce_motion:false,reduce_shake:false,screen_reader:false)",
        ),
        enumeration(
            "klotho_ir::ContrastMode",
            [
                ("default", 0, "production palette"),
                ("high", 1, "high-contrast tokens"),
            ]
            .into_iter()
            .map(|(n, d, desc)| variant(n, Some(d), "unit", desc))
            .collect(),
            "default",
        ),
        enumeration(
            "klotho_ir::CaptionMode",
            [
                ("off", 0, "no caption band"),
                ("subtitles", 1, "dialogue subtitles"),
                ("closed_captions", 2, "subtitles plus SDH"),
            ]
            .into_iter()
            .map(|(n, d, desc)| variant(n, Some(d), "unit", desc))
            .collect(),
            "subtitles",
        ),
        enumeration(
            "klotho_manifest::FocusRole",
            [
                ("menu", 0, "top-level sheet"),
                ("item", 1, "focusable row"),
                ("button", 2, "activate control"),
                ("slider", 3, "bounded numeric control"),
                ("toggle", 4, "binary control"),
                ("group", 5, "group header"),
                ("caption", 6, "subtitle/CC band"),
                ("status", 7, "HUD status"),
            ]
            .into_iter()
            .map(|(n, d, desc)| variant(n, Some(d), "unit", desc))
            .collect(),
            "button",
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
        structure(
            "klotho_eval::JourneySpec",
            vec![
                field("id", "JourneyId", "stable journey identity"),
                field("start", "StartStateRef", "named start fixture"),
                field("steps", "Vec<JourneyStep>", "public-input steps"),
                field("assertions", "Vec<JourneyAssertion>", "semantic facts"),
                field("capture_points", "Vec<CapturePoint>", "capture markers"),
                bounded("max_ticks", "u32", "hard tick cap", 0, 1_000_000),
                field("depends_on", "Vec<JourneyId>", "prerequisites"),
                field("anchors", "Vec<AnchorId>", "covered anchors"),
                field("modules", "Vec<AnchorId>", "covered modules"),
            ],
            "(id:\"unlock\",start:(name:\"default\"),steps:[],assertions:[],capture_points:[],max_ticks:16,depends_on:[],anchors:[],modules:[])",
            "(id:\"\",start:(),steps:[])",
        ),
        enumeration(
            "klotho_eval::JourneyStep",
            [
                ("Device", "device sample; no Agency"),
                ("Fixture", "verb fixture; adapter stamps Agency"),
                ("Wait", "ticks with no player packet"),
                ("Camera", "presentation-only camera move"),
                ("Save", "save slot"),
                ("Load", "load slot"),
            ]
            .into_iter()
            .map(|(n, desc)| variant(n, None, "payload", desc))
            .collect(),
            "Wait(ticks:1)",
        ),
        enumeration(
            "klotho_eval::JourneyAssertion",
            [
                ("Trace", "admitted Trace body token"),
                ("Qty", "quantity comparison"),
                ("Rel", "relation triple"),
                ("Knows", "mind fact"),
                ("Place", "Rel::In residency"),
                ("Capture", "named capture was recorded"),
            ]
            .into_iter()
            .map(|(n, desc)| variant(n, None, "payload", desc))
            .collect(),
            "Place(locus:\"player\",place:\"hall\")",
        ),
        structure(
            "klotho_eval::EvidenceBundle",
            vec![
                field("change", "Hash", "change identity"),
                field("project_hash", "Hash", "authoring project"),
                field("toolchain_hash", "Hash", "toolchain lock"),
                field("expanded_ir_hash", "Hash", "expanded IR"),
                field("canon_hash", "Hash", "cooked Canon"),
                field("cas_root", "Hash", "CAS root"),
                field("checks", "Vec<CheckEvidence>", "trusted checks"),
                field("captures", "Vec<ArtifactRef>", "captures"),
                field("approvals", "Vec<ApprovalRef>", "human approvals"),
                field("signature", "Hash", "evidence_signature of payload"),
            ],
            "(change:\"0000000000000000000000000000000000000000000000000000000000000000\",project_hash:\"0000000000000000000000000000000000000000000000000000000000000000\",toolchain_hash:\"0000000000000000000000000000000000000000000000000000000000000000\",expanded_ir_hash:\"0000000000000000000000000000000000000000000000000000000000000000\",canon_hash:\"0000000000000000000000000000000000000000000000000000000000000000\",cas_root:\"0000000000000000000000000000000000000000000000000000000000000000\",checks:[],captures:[],approvals:[],signature:\"0000000000000000000000000000000000000000000000000000000000000000\")",
            "(change:\"00\")",
        ),
        structure(
            "klotho_eval::AcceptanceContract",
            vec![
                field("claims", "Vec<SemanticClaim>", "semantic claims"),
                field("journeys", "Vec<JourneyId>", "required journeys"),
                field("invariants", "Vec<InvariantRef>", "catalog invariants"),
                field("quality", "Vec<QualityTarget>", "quality targets"),
                field("budgets", "Vec<BudgetTarget>", "budget targets"),
                field("non_regression", "Vec<JourneyId>", "must not regress"),
                field("allowed_scope", "ChangeScope", "write scope"),
            ],
            "(claims:[],journeys:[],invariants:[],quality:[],budgets:[],non_regression:[],allowed_scope:(modules:[],anchors:[]))",
            "(claims:())",
        ),
        structure(
            "klotho_dcc::AssetRequest",
            vec![
                field("id", "AssetRequestId", "content-derived request identity"),
                field("role", "AssetRole", "production role"),
                field("semantic_tag", "String", "required semantic binding tag"),
                field("references", "Vec<Hash>", "approved reference hashes"),
                field("dimensions_mm", "BoundsMm", "integer dimensions"),
                field("visual_budget", "VisualBudget", "geometry/texture caps"),
                field("material_budget", "MaterialBudget", "material/shader caps"),
                field("rig", "Option<RigContract>", "optional rig contract"),
                field("lods", "LodContract", "ordered LOD caps"),
                field("collision", "CollisionRequest", "semantic hull policy"),
                field("variants", "u16", "requested variants"),
                field("platform_tiers", "BTreeSet<GpuTier>", "target GPU tiers"),
                field("routes", "BTreeSet<SourceRoute>", "allowed intake routes"),
            ],
            "(id:\"0000000000000000000000000000000000000000000000000000000000000000\",role:prop,semantic_tag:\"prop.crate\",references:[\"1111111111111111111111111111111111111111111111111111111111111111\"],dimensions_mm:(x:500,y:500,z:500),visual_budget:(triangles:10000,vertices:10000,texture_bytes:16777216),material_budget:(slots:2,textures:4,shader_features:4),rig:None,lods:(levels:3,max_triangles:[10000,5000,1000]),collision:proposed_hull,variants:1,platform_tiers:[desktop_high],routes:[retrieval,vendor])",
            "(id:\"00\",role:prop,semantic_tag:\"\",references:[],dimensions_mm:(x:0,y:0,z:0),visual_budget:(triangles:0,vertices:0,texture_bytes:0),material_budget:(slots:0,textures:0,shader_features:0),rig:None,lods:(levels:0,max_triangles:[]),collision:none,variants:0,platform_tiers:[],routes:[])",
        ),
        structure(
            "klotho_prove::ReleaseRights",
            vec![
                field(
                    "route",
                    "RightsRoute",
                    "retrieval/generated/vendor/commissioned",
                ),
                field("origin", "Hash", "origin record"),
                field("terms", "Hash", "license/contract terms"),
                field("ownership", "Hash", "output ownership representation"),
                field("indemnity", "Hash", "indemnity position"),
                field("source_permission", "Hash", "source/reference permission"),
                field("consent", "Hash", "performer/likeness or N/A decision"),
                field(
                    "restrictions",
                    "Hash",
                    "territory/union/export/trademark review",
                ),
                field("approved_by", "String", "named legal approver"),
                field("approval", "Hash", "signed approval record"),
            ],
            "(route:vendor,origin:\"1111111111111111111111111111111111111111111111111111111111111111\",terms:\"2222222222222222222222222222222222222222222222222222222222222222\",ownership:\"3333333333333333333333333333333333333333333333333333333333333333\",indemnity:\"4444444444444444444444444444444444444444444444444444444444444444\",source_permission:\"5555555555555555555555555555555555555555555555555555555555555555\",consent:\"6666666666666666666666666666666666666666666666666666666666666666\",restrictions:\"7777777777777777777777777777777777777777777777777777777777777777\",approved_by:\"legal.owner\",approval:\"8888888888888888888888888888888888888888888888888888888888888888\")",
            "(route:vendor,origin:\"0000000000000000000000000000000000000000000000000000000000000000\")",
        ),
        structure(
            "klotho_dialogue::StoryBible",
            vec![
                field("anchor", "AnchorId", "immutable bible identity"),
                field(
                    "version",
                    "u32",
                    "explicit version; bump invalidates evidence",
                ),
                field("characters", "Vec<CharacterFact>", "voice and facts"),
                field("timeline", "Vec<TimelineBeat>", "ordered beats"),
                field("locations", "Vec<LocationFact>", "named places"),
                field("glossary", "Vec<GlossaryEntry>", "terms loc must cite"),
                field("secrets", "Vec<SecretFact>", "Knows-gated secrets"),
                field("themes", "Vec<ThemeFact>", "writing themes"),
                field("rating", "RatingLimits", "content limits"),
                field("unresolved", "Vec<UnresolvedQuestion>", "open questions"),
                field("exceptions", "Vec<ApprovedException>", "named exceptions"),
                field("presence", "Vec<Presence>", "character location per beat"),
            ],
            "(anchor:\"00000000000000000000000000000000\",version:1,characters:[],timeline:[],locations:[],glossary:[],secrets:[],themes:[],rating:(board:\"esrb_t\",forbid:[]),unresolved:[],exceptions:[],presence:[])",
            "(anchor:\"00\",version:0,characters:[],timeline:[],locations:[],glossary:[],secrets:[],themes:[],rating:(board:\"\",forbid:[]),unresolved:[],exceptions:[],presence:[])",
        ),
        structure(
            "klotho_dialogue::QuestGraph",
            vec![
                field("anchor", "AnchorId", "immutable graph identity"),
                field("quests", "Vec<QuestNode>", "nodes; validate sorts by id"),
                field("exclusions", "Vec<QuestExclusion>", "mutual exclusions"),
            ],
            "(anchor:\"00000000000000000000000000000000\",quests:[],exclusions:[])",
            "(anchor:\"00\",quests:(),exclusions:[])",
        ),
        structure(
            "klotho_dialogue::QuestNode",
            vec![
                field("anchor", "AnchorId", "immutable identity"),
                field("id", "Name", "quest id"),
                field("prerequisites", "Vec<Name>", "quests or Knows facts"),
                field("grants", "Vec<Name>", "Knows grants"),
                field("failure", "Option<Name>", "failure successor"),
                field("cancel", "Option<Name>", "cancel successor"),
                field("reentry", "ReentryKind", "save/re-entry"),
                field("critical", "bool", "critical-path membership"),
                field("available_at_start", "bool", "offered with no prereq"),
                field("escape", "bool", "authored cycle escape"),
                field("ending", "bool", "terminal node"),
            ],
            "(anchor:\"00000000000000000000000000000000\",id:\"decode_plates\",prerequisites:[],grants:[\"plates_decoded\"],failure:None,cancel:None,reentry:checkpoint,critical:true,available_at_start:true,escape:false,ending:false)",
            "(anchor:\"00\",id:\"\",prerequisites:[],grants:[],failure:None,cancel:None,reentry:never,critical:false,available_at_start:false,escape:false,ending:false)",
        ),
        structure(
            "klotho_dialogue::DialogueModule",
            vec![
                field("anchor", "AnchorId", "immutable module identity"),
                field("id", "Name", "conversation id"),
                field("entry", "Name", "entry line key"),
                field(
                    "lines",
                    "Vec<DialogueLine>",
                    "stable keys; review is narrative order",
                ),
            ],
            "(anchor:\"00000000000000000000000000000000\",id:\"mira_observatory\",entry:\"mira.greet\",lines:[])",
            "(anchor:\"00\",id:\"\",entry:\"\",lines:[])",
        ),
        structure(
            "klotho_dialogue::DialogueLine",
            vec![
                field("anchor", "AnchorId", "immutable identity"),
                field("key", "Name", "stable localization key"),
                field("speaker", "Name", "bible character"),
                field("beat", "Name", "timeline beat"),
                field("condition", "DialogueCond", "Knows/quest condition"),
                field("grants", "Vec<Name>", "Knows granted on play"),
                field("choices", "Vec<DialogueChoice>", "player choices"),
                field("timing", "LineTiming", "VO/subtitle ticks"),
                field("performance", "String", "notes; never executed"),
                field("cc", "ClosedCaption", "required SDH/CC"),
                field("source_text", "String", "source-locale body"),
                field("vo", "Option<VoBinding>", "approved grain + rights"),
                field("vo_required", "bool", "release requires VO"),
                field("next", "Option<Name>", "linear successor"),
            ],
            "(anchor:\"00000000000000000000000000000000\",key:\"mira.greet\",speaker:\"mira\",beat:\"arrival\",condition:always,grants:[],choices:[],timing:(start:0,duration:12),performance:\"\",cc:(speaker:\"mira\",body:\"Hi\",sdh:true),source_text:\"Hi\",vo:None,vo_required:false,next:None)",
            "(anchor:\"00\",key:\"\",speaker:\"\",beat:\"\",condition:always,grants:[],choices:[],timing:(start:0,duration:0),performance:\"\",cc:(speaker:\"\",body:\"\",sdh:false),source_text:\"\",vo:None,vo_required:true,next:None)",
        ),
        structure(
            "klotho_dialogue::LocaleCatalog",
            vec![
                field("locale", "LocaleId", "BCP-47 like id"),
                field("strings", "BTreeMap<Name,Message>", "ICU-style messages"),
                field(
                    "approval",
                    "Option<LinguisticApproval>",
                    "shipping approval",
                ),
                field("font", "FontContract", "shaping and fallbacks"),
            ],
            "(locale:\"en\",strings:{},approval:None,font:(locale:\"en\",family:\"klotho-sans\",shaping:ltr,fallbacks:[],controller_glyphs:\"xbox\"))",
            "(locale:\"\",strings:{},approval:None,font:(locale:\"\",family:\"\",shaping:ltr,fallbacks:[],controller_glyphs:\"\"))",
        ),
        structure(
            "klotho_dialogue::Message",
            vec![
                field("key", "Name", "line or choice key"),
                field("pattern", "String", "named {placeholders} only"),
                field("gender", "Option<Gender>", "agreement metadata"),
                field("plural", "Option<PluralForm>", "plural metadata"),
                field("context", "String", "translator context"),
            ],
            "(key:\"mira.greet\",pattern:\"Hello\",gender:Some(neutral),plural:Some(other),context:\"observatory\")",
            "(key:\"\",pattern:\"${eval}\",gender:None,plural:None,context:\"\")",
        ),
        structure(
            "klotho_dialogue::FontContract",
            vec![
                field("locale", "LocaleId", "covered locale"),
                field("family", "Name", "primary family"),
                field("shaping", "ShapingScript", "ltr/rtl/cjk/complex"),
                field("fallbacks", "Vec<Name>", "ordered fallbacks"),
                field("controller_glyphs", "Name", "glyph set"),
            ],
            "(locale:\"ja\",family:\"klotho-sans\",shaping:cjk,fallbacks:[\"klotho-fallback\"],controller_glyphs:\"xbox\")",
            "(locale:\"\",family:\"\",shaping:ltr,fallbacks:[],controller_glyphs:\"\")",
        ),
        enumeration(
            "klotho_dialogue::ReentryKind",
            [
                ("never", 0, "no re-entry"),
                ("checkpoint", 1, "last checkpoint"),
                ("always", 2, "always after cancel"),
            ]
            .into_iter()
            .map(|(n, d, desc)| variant(n, Some(d), "unit", desc))
            .collect(),
            "checkpoint",
        ),
        enumeration(
            "klotho_dialogue::ShapingScript",
            [
                ("ltr", 0, "left-to-right"),
                ("rtl", 1, "right-to-left"),
                ("cjk", 2, "CJK"),
                ("complex", 3, "complex shaping"),
            ]
            .into_iter()
            .map(|(n, d, desc)| variant(n, Some(d), "unit", desc))
            .collect(),
            "ltr",
        ),
        enumeration(
            "klotho_dialogue::DialogueCond",
            vec![
                variant("always", None, "unit", "unconditional"),
                variant("knows", None, "Name", "Knows fact"),
                variant("quest", None, "Name", "completed quest"),
                variant("all", None, "Vec<DialogueCond>", "conjunction"),
                variant("any", None, "Vec<DialogueCond>", "disjunction"),
                variant("not", None, "DialogueCond", "negation"),
            ],
            "always",
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

fn pattern_schemas() -> Vec<PatternSchema> {
    klotho_pattern::catalog_rows()
        .into_iter()
        .map(|row| PatternSchema {
            id: row.id,
            version: row.version,
            family: row.family,
            parameters: row.parameters,
            requires: row.requires,
            grants: row.grants,
            conflicts: row.conflicts,
            predicates: row.predicates,
            rite_steps: row.rite_steps,
            per_tick: row.per_tick,
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
        ("author.set_feel@1", "FeelContract", &["feel"], 2),
        ("asset.request@1", "AssetRequest", &["asset_queue"], 5),
        (
            "author.feel_sweep@1",
            "{action:Name,candidates:Vec<FeelContract>}",
            &["review"],
            2,
        ),
        ("world.plan@1", "WorldPlan", &["places", "graph"], 4),
        ("world.dress@1", "Vec<DressingInstance>", &["dressing"], 2),
        ("narrative.bible.set@1", "StoryBible", &["bible"], 3),
        ("narrative.quest.graph@1", "QuestGraph", &["quests"], 3),
        (
            "narrative.dialogue.module@1",
            "DialogueModule",
            &["dialogue"],
            4,
        ),
        ("narrative.loc.catalog@1", "LocaleCatalog", &["locales"], 2),
        ("author.set_a11y@1", "A11yProfile", &["a11y"], 2),
        ("input.remap@1", "{verb:Verb,button:Button}", &["binds"], 1),
        ("ui.layout.set@1", "UiNode", &["ui"], 2),
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
        Agency, Analog, AssistLevel, Beat, CanonDiff, Channel, Cost, IntentDoc, MindProgram,
        MindSpec, Name, Pred, SeedFact, Slot, StyleIntent,
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
        assert_eq!(catalog.patterns.len(), 51);
        assert!(
            catalog
                .patterns
                .iter()
                .any(|p| p.id == "traversal.door_key" && p.version == 2)
        );
        assert!(
            catalog
                .patterns
                .iter()
                .any(|p| p.id == "feel.action_contract" && p.family == "feel")
        );
        assert!(
            catalog
                .kinds
                .iter()
                .any(|k| k.id == "klotho_dialogue::StoryBible")
        );
        assert!(
            catalog
                .operations
                .iter()
                .any(|o| o.id == "narrative.dialogue.module@1")
        );
        assert!(
            catalog
                .kinds
                .iter()
                .any(|k| k.id == "klotho_ir::A11yProfile")
        );
        assert!(
            catalog
                .operations
                .iter()
                .any(|o| o.id == "author.set_a11y@1")
        );
        assert!(
            catalog
                .patterns
                .iter()
                .any(|p| p.id == "ui.menu_focus" && p.family == "ui")
        );
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
        round_trip(&klotho_ir::FeelContract::spindle_use());
        round_trip(&klotho_ir::A11yProfile::first_title());
        round_trip(&klotho_ir::ContrastMode::High);
        round_trip(&klotho_ir::CaptionMode::ClosedCaptions);
        round_trip(&klotho_pattern::greybox_route());
        round_trip(&klotho_pattern::PlaceBudgets::greybox());
        round_trip(&klotho_dialogue::observatory());
        round_trip(&klotho_dialogue::ReentryKind::Checkpoint);
        round_trip(&klotho_dialogue::ShapingScript::Cjk);
        round_trip(&Cost {
            res: Name::from("stamina"),
            amount: 1,
        });
        round_trip(&MindSpec {
            locus: name.clone(),
            program: MindProgram::default(),
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
