//! Distaff errors. Parse failures wrap [`klotho_ir::IrError`].

use core::fmt;

use klotho_compile::CompileError;
use klotho_ir::IrError;

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
}

impl fmt::Display for AuthorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ir(e) => write!(f, "{e}"),
            Self::Cook(e) => write!(f, "{e}"),
            Self::Io(s) => write!(f, "{s}"),
            Self::EmptyPinReason => write!(f, "pin reason must be non-empty"),
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
