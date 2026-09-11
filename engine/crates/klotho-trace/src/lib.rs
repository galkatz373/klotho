//! Append-only Trace: the history of committed facts (K19).
//!
//! `World` is a view of `(canon_hash, trace_prefix_hash)` plus the live Intent
//! heap. This crate owns the log, the prefix hash, and the per-tick
//! [`TraceDelta`]. Encoding is canonical little-endian; hashed bytes never go
//! through serde. Replay equality is same events ⇒ same prefix.
//!
//! Depends on `klotho-core` and `klotho-prove` only (PR 05).
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod delta;
mod encode;
mod error;
mod event;
mod log;

pub use delta::TraceDelta;
pub use encode::{EVENT_VERSION, decode_event, encode_event};
pub use error::TraceError;
pub use event::{
    ISLAND_SNAP_PERIOD_TICKS, IslandSnap, PoseReason, ProposalKind, RelTag, RiteEnd, TraceBody,
    TraceEvent,
};
pub use klotho_core::Hash;
pub use log::{GENESIS_DOMAIN, TraceLog, fold_prefix, genesis_hash};
