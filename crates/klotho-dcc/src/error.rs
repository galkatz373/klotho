//! Cook-time glTF failures. None of these are `KernelFault`.

use core::fmt;

use klotho_compile::CompileError;

/// Why a glTF failed to cook.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum DccError {
    /// A contributing mesh/node has no `extras.klotho.affordance`.
    MissingTag(String),
    /// A millimetre extent does not fit in quantized `i16` verts.
    QuantizeOverflow,
    /// glTF 2.0 subset rejected (mode, accessor, animation, …).
    Gltf(String),
    /// SPDX extras and sidecar were missing, or SPDX id was empty.
    License(String),
    /// Encode / validate of a KLTH blob failed.
    Compile(CompileError),
    /// Filesystem read failed.
    Io(String),
}

impl fmt::Display for DccError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingTag(t) => write!(f, "MissingTag({t})"),
            Self::QuantizeOverflow => write!(f, "QuantizeOverflow"),
            Self::Gltf(s) => write!(f, "Gltf({s})"),
            Self::License(s) => write!(f, "License({s})"),
            Self::Compile(e) => write!(f, "{e}"),
            Self::Io(s) => write!(f, "Io({s})"),
        }
    }
}

impl core::error::Error for DccError {}

impl From<CompileError> for DccError {
    fn from(e: CompileError) -> Self {
        match e {
            CompileError::QuantizeOverflow => Self::QuantizeOverflow,
            CompileError::MissingTag(t) => Self::MissingTag(t),
            other => Self::Compile(other),
        }
    }
}
