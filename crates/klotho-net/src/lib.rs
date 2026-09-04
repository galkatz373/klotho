//! Listen-server and dedicated server: signed [`PlayerIntent`], TraceDelta,
//! Interest codebook, and unhashed [`Packet::PoseDelta`] for the client overlay.
//!
//! Role::Host is the 2-player listen profile (kernel on this process, 20 Hz).
//! Role::Server is dedicated (many clients, kernel on this process).
//! Role::Client is overlay-only and does not run CommitKernel.
//!
//! Overlay, PoseDelta, and rewind state are never hashed and never written to
//! Trace. Signatures authenticate which client sent [`PlayerIntent`], not
//! whether a human produced it. Mind and Infer run on Host and Server only.
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod error;
mod overlay;
mod packet;
mod replay;
mod server;
mod session;
mod sign;

pub use error::NetError;
pub use overlay::{DEFAULT_OVERLAY_SNAP_MM, Overlay};
pub use packet::{
    CompilerStamp, InterestDict, MAX_BLOB, MAX_EVENTS, MAX_INTENT, MAX_INTEREST, MAX_PACKET,
    MAX_POSE_DELTA, Packet, PoseBlock, PoseDeltaEntry, PoseFull, STAMP_TOKEN, SnapshotBlob,
    decode_frame, decode_packet, decode_player_intent, encode_frame, encode_packet,
    encode_player_intent,
};
pub use replay::{ReplayFile, load_replay, load_replay_intents, write_replay};
pub use server::{MAX_DEDICATED_PLAYERS, Server, dedicated_session, dedicated_session_at};
pub use session::{Client, DisconnectReason, Host, LISTEN_INTENT_HZ, Role, Wire, memory_session};
pub use sign::{Keypair, Signed, sign_intent, verify_bytes, verify_intent};

pub use klotho_ir::PlayerIntent;
pub use klotho_trace::{TraceEvent, fold_prefix, genesis_hash};

#[cfg(test)]
mod tests {
    #[test]
    fn no_predicted_in_packets() {
        let needle = concat!("Pred", "icted");
        for src in [
            include_str!("packet.rs"),
            include_str!("session.rs"),
            include_str!("overlay.rs"),
            include_str!("sign.rs"),
            include_str!("replay.rs"),
            include_str!("error.rs"),
            include_str!("server.rs"),
        ] {
            assert!(
                !src.contains(needle),
                "that bit does not exist on Trace or the wire"
            );
        }
    }
}
