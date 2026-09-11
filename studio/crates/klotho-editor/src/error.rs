//! Distaff editor failures. Pin / cook wrap [`klotho_author::AuthorError`].

use core::fmt;

use klotho_ai::AiError;
use klotho_author::AuthorError;
use klotho_core::KernelFault;
use klotho_ir::Name;

/// Load / cook / Pin / play failure.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum EditorError {
    /// Parse, Pin, or cook from `klotho-author`.
    Author(AuthorError),
    /// Isolated authoring transaction.
    Ai(AiError),
    /// Seed apply or kernel boot from cooked Canon.
    Boot(String),
    /// Viewport / play / dashboard needs a successful cook.
    NoCook,
    /// Named locus is not in the document seed.
    UnknownLocus(Name),
    /// Pin of a pose needs a gizmo overlay for that locus.
    NoOverlay(Name),
    /// Kernel invariant during `step`.
    Fault(KernelFault),
}

impl fmt::Display for EditorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Author(e) => write!(f, "{e}"),
            Self::Ai(e) => write!(f, "{e}"),
            Self::Boot(s) => write!(f, "{s}"),
            Self::NoCook => write!(f, "no cook"),
            Self::UnknownLocus(n) => write!(f, "unknown locus {n}"),
            Self::NoOverlay(n) => write!(f, "no overlay pose {n}"),
            Self::Fault(e) => write!(f, "{e}"),
        }
    }
}

impl core::error::Error for EditorError {}

impl From<AuthorError> for EditorError {
    fn from(e: AuthorError) -> Self {
        Self::Author(e)
    }
}

impl From<AiError> for EditorError {
    fn from(e: AiError) -> Self {
        Self::Ai(e)
    }
}

impl From<klotho_ir::IrError> for EditorError {
    fn from(e: klotho_ir::IrError) -> Self {
        Self::Author(AuthorError::from(e))
    }
}

impl From<KernelFault> for EditorError {
    fn from(e: KernelFault) -> Self {
        Self::Fault(e)
    }
}
