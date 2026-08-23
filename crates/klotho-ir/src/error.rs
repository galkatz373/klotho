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
        }
    }
}

impl core::error::Error for IrError {}
