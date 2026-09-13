//! Parse and validation failures. None of these are `KernelFault`.

use core::fmt;

/// IR parse / validate error. Cook maps these to cook-fail.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum IrError {
    /// RON parse failed.
    Parse(String),
    /// RON serialize failed.
    Ser(String),
    /// A [`crate::Name`] (or equivalent string id) was empty.
    EmptyName,
    /// `ExistsRelated` / `CountRelated` nested inside another quantifier.
    NestedQuantifier,
    /// `cap_steps` or `cap_ticks` was zero.
    InvalidRiteCap,
    /// Analog phase outside `0..=1000`.
    InvalidPhase(u16),
    /// Duplicate entry in `Agency.claimed`.
    DuplicateChannel,
    /// Module `version` was zero.
    InvalidModuleVersion,
    /// Module import graph contains a cycle. Names are sorted.
    ImportCycle(Vec<String>),
    /// Locked or imported content hash does not match module bytes.
    HashDrift {
        /// Module id.
        id: String,
        /// Hash recorded in the lock or import.
        expected: String,
        /// Hash of the loaded module.
        actual: String,
    },
    /// Two modules share an id.
    DuplicateModule(String),
    /// An import or lock entry names a module that is not loaded.
    MissingModule(String),
    /// A live object reuses a tombstoned name.
    TombstoneReuse(String),
    /// An alias collides with a live primary name.
    AliasCollision(String),
    /// A module parameter has no default and was not bound.
    UnboundParameter(String),
    /// Two objects were assigned the same [`crate::AnchorId`].
    DuplicateAnchor(String),
    /// A named live object has no immutable anchor.
    MissingAnchor(String),
    /// Two live objects in the flattened project share a name.
    DuplicateObjectName(String),
    /// An export names an object the module does not define.
    ExportUnknown(String),
    /// A parameter default does not match its declared type.
    ParameterTypeMismatch(String),
    /// A pattern instance is still present at flatten time.
    UnexpandedPattern(String),
    /// A [`crate::FeelContract`] field is out of bounds.
    InvalidFeel {
        /// Field path (`input_buffer_ticks`).
        field: String,
        /// Why it failed.
        reason: String,
    },
    /// A compiled Mind program exceeds a K90 content-independent cap.
    MindProgramCap {
        /// Actor anchor/name.
        locus: String,
        /// Capped table.
        resource: String,
        /// Authored count.
        actual: usize,
        /// Hard cap.
        cap: usize,
    },
    /// A compiled Mind program contains an invalid local reference.
    InvalidMindProgram(String),
    /// A Far table reads or affects a protected fact.
    UnsafeFarFact {
        /// Actor anchor/name.
        locus: String,
        /// Protected fact id.
        fact: String,
    },
}

impl fmt::Display for IrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(s) => write!(f, "Parse({s})"),
            Self::Ser(s) => write!(f, "Ser({s})"),
            Self::EmptyName => write!(f, "EmptyName"),
            Self::NestedQuantifier => write!(f, "NestedQuantifier"),
            Self::InvalidRiteCap => write!(f, "InvalidRiteCap"),
            Self::InvalidPhase(p) => write!(f, "InvalidPhase({p})"),
            Self::DuplicateChannel => write!(f, "DuplicateChannel"),
            Self::InvalidModuleVersion => write!(f, "InvalidModuleVersion"),
            Self::ImportCycle(ids) => write!(f, "ImportCycle({})", ids.join(",")),
            Self::HashDrift {
                id,
                expected,
                actual,
            } => write!(f, "HashDrift({id}: expected {expected}, got {actual})"),
            Self::DuplicateModule(id) => write!(f, "DuplicateModule({id})"),
            Self::MissingModule(id) => write!(f, "MissingModule({id})"),
            Self::TombstoneReuse(name) => write!(f, "TombstoneReuse({name})"),
            Self::AliasCollision(name) => write!(f, "AliasCollision({name})"),
            Self::UnboundParameter(name) => write!(f, "UnboundParameter({name})"),
            Self::DuplicateAnchor(id) => write!(f, "DuplicateAnchor({id})"),
            Self::MissingAnchor(name) => write!(f, "MissingAnchor({name})"),
            Self::DuplicateObjectName(name) => write!(f, "DuplicateObjectName({name})"),
            Self::ExportUnknown(name) => write!(f, "ExportUnknown({name})"),
            Self::ParameterTypeMismatch(name) => write!(f, "ParameterTypeMismatch({name})"),
            Self::UnexpandedPattern(id) => write!(f, "UnexpandedPattern({id})"),
            Self::InvalidFeel { field, reason } => write!(f, "InvalidFeel({field}: {reason})"),
            Self::MindProgramCap {
                locus,
                resource,
                actual,
                cap,
            } => {
                write!(f, "MindProgramCap({locus}.{resource}: {actual} > {cap})")
            }
            Self::InvalidMindProgram(reason) => write!(f, "InvalidMindProgram({reason})"),
            Self::UnsafeFarFact { locus, fact } => write!(f, "UnsafeFarFact({locus}.{fact})"),
        }
    }
}

impl core::error::Error for IrError {}
