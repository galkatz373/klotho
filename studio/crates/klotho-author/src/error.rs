//! Distaff errors. Parse failures wrap [`klotho_ir::IrError`].

use core::fmt;

use klotho_compile::CompileError;
use klotho_ir::IrError;
use klotho_pattern::PatternError;

/// Load / Pin / cook failure.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum AuthorError {
    /// RON / kdown parse or structural validation.
    Ir(IrError),
    /// Kitbash retrieval or Canon cook.
    Cook(CompileError),
    /// Filesystem read.
    Io(String),
    /// Pin requires a non-empty reason.
    EmptyPinReason,
    /// Remove / alias reason was empty.
    EmptyReason,
    /// No live object or module has this identity.
    MissingAnchor(String),
    /// An identity is already assigned.
    DuplicateAnchor(String),
    /// A live object already uses this name.
    NameInUse(String),
    /// A tombstoned name cannot be reused.
    TombstoneReuse(String),
    /// An alias collides with a live or reserved name.
    AliasCollision(String),
    /// No module with this identity is loaded.
    ModuleNotFound(String),
    /// Pattern expansion failed.
    Pattern(String),
}

impl fmt::Display for AuthorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ir(e) => write!(f, "{e}"),
            Self::Cook(e) => write!(f, "{e}"),
            Self::Io(s) => write!(f, "{s}"),
            Self::EmptyPinReason => write!(f, "pin reason must be non-empty"),
            Self::EmptyReason => write!(f, "reason must be non-empty"),
            Self::MissingAnchor(id) => write!(f, "missing anchor {id}"),
            Self::DuplicateAnchor(id) => write!(f, "duplicate anchor {id}"),
            Self::NameInUse(name) => write!(f, "name in use {name}"),
            Self::TombstoneReuse(name) => write!(f, "tombstone reuse {name}"),
            Self::AliasCollision(name) => write!(f, "alias collision {name}"),
            Self::ModuleNotFound(id) => write!(f, "module not found {id}"),
            Self::Pattern(s) => write!(f, "{s}"),
        }
    }
}

impl core::error::Error for AuthorError {}

impl From<IrError> for AuthorError {
    fn from(e: IrError) -> Self {
        Self::Ir(e)
    }
}

impl From<CompileError> for AuthorError {
    fn from(e: CompileError) -> Self {
        Self::Cook(e)
    }
}

impl From<PatternError> for AuthorError {
    fn from(e: PatternError) -> Self {
        Self::Pattern(e.to_string())
    }
}
