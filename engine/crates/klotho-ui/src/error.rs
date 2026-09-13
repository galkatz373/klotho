//! Presentation UI failures. Never a kernel fault.

use core::fmt;

/// Why a layout, focus path, remap, or compliance check failed.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum UiError {
    /// A leaf did not fit the safe area or its own bounds.
    Overflow {
        /// Node id.
        node: String,
        /// Locale under test.
        locale: String,
    },
    /// Two non-spacer leaves occupy the same pixels.
    Overlap {
        /// First node.
        a: String,
        /// Second node.
        b: String,
    },
    /// A focusable node was never visited.
    IncompleteFocus {
        /// Node id.
        missing: String,
    },
    /// A required accessibility row is missing.
    Compliance {
        /// Requirement id.
        requirement: String,
        /// Witness.
        witness: String,
    },
}

impl fmt::Display for UiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Overflow { node, locale } => write!(f, "Overflow({node}@{locale})"),
            Self::Overlap { a, b } => write!(f, "Overlap({a},{b})"),
            Self::IncompleteFocus { missing } => write!(f, "IncompleteFocus({missing})"),
            Self::Compliance {
                requirement,
                witness,
            } => write!(f, "Compliance({requirement}: {witness})"),
        }
    }
}

impl core::error::Error for UiError {}
