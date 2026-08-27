//! Listen-server: the host runs the only CommitKernel; clients send signed
//! [`PlayerIntent`] at 20 Hz and delay-interpolate a client-only overlay from
//! [`Packet::TraceDelta`].
//!
//! 20 Hz intent with no prediction is Hearth-adequate, not shooter-adequate.
//! Signatures authenticate which client sent [`PlayerIntent`], not whether a
//! human produced it. The overlay is never hashed and is discarded on each
//! delta. Mind and Infer run on the host only.
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod error;
mod overlay;
mod packet;
mod replay;
mod session;
mod sign;

pub use error::NetError;
pub use overlay::Overlay;
pub use packet::{
    CompilerStamp, MAX_BLOB, MAX_EVENTS, MAX_INTENT, MAX_PACKET, Packet, STAMP_TOKEN, SnapshotBlob,
    decode_frame, decode_packet, decode_player_intent, encode_frame, encode_packet,
    encode_player_intent,
};
pub use replay::{ReplayFile, load_replay, load_replay_intents, write_replay};
pub use session::{Client, DisconnectReason, Host, Role, Wire, memory_session};
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
        ] {
            assert!(
                !src.contains(needle),
                "that bit does not exist on Trace or the wire"
            );
        }
    }
}
