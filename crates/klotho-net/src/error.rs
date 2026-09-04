//! Wire and session failures. None of these are `KernelFault`.

use core::fmt;

/// Why a packet, signature, or session transition failed.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum NetError {
    /// Frame or field ended before the declared length.
    Truncated,
    /// Length, event count, or blob exceeded a documented cap.
    Oversize,
    /// Packet tag is not in the frozen v1 set.
    UnknownTag,
    /// PlayerIntent bytes are not canonical LE / fail structural decode.
    BadIntent,
    /// A TraceEvent inside a delta failed to decode.
    BadEvent,
    /// Ed25519 verify failed. Packet is dropped, not ingested.
    BadSignature,
    /// Verifying-key bytes are the wrong length, off-curve, or small-order.
    BadKey,
    /// OS entropy for join-time keygen failed.
    Keygen,
    /// Hello `canon_hash`, `epoch`, or [`crate::CompilerStamp`] did not match.
    HelloMismatch,
    /// Listen-server is two players (`PlayerId` 0 host, 1 remote).
    ThirdPlayer,
    /// Dedicated server already holds [`crate::MAX_DEDICATED_PLAYERS`].
    ServerFull,
    /// Trace prefix / delta ancestry mismatch. Disconnect and write a replay.
    Desync,
    /// Space, Motion, Mind, and Infer run on the host only.
    HostOnly,
    /// Intent sent before Hello completed.
    NotJoined,
    /// Session already disconnected.
    Disconnected,
    /// Replay file read/write failed.
    Io(String),
    /// Replay RON failed to parse or serialize.
    Replay(String),
}

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => write!(f, "Truncated"),
            Self::Oversize => write!(f, "Oversize"),
            Self::UnknownTag => write!(f, "UnknownTag"),
            Self::BadIntent => write!(f, "BadIntent"),
            Self::BadEvent => write!(f, "BadEvent"),
            Self::BadSignature => write!(f, "BadSignature"),
            Self::BadKey => write!(f, "BadKey"),
            Self::Keygen => write!(f, "Keygen"),
            Self::HelloMismatch => write!(f, "HelloMismatch"),
            Self::ThirdPlayer => write!(f, "ThirdPlayer"),
            Self::ServerFull => write!(f, "ServerFull"),
            Self::Desync => write!(f, "Desync"),
            Self::HostOnly => write!(f, "HostOnly"),
            Self::NotJoined => write!(f, "NotJoined"),
            Self::Disconnected => write!(f, "Disconnected"),
            Self::Io(s) => write!(f, "Io({s})"),
            Self::Replay(s) => write!(f, "Replay({s})"),
        }
    }
}

impl core::error::Error for NetError {}
