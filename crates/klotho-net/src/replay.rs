//! Desync disconnect writes a replay of host-ingested intents.

use std::fs;
use std::path::Path;

use klotho_core::Hash;
use klotho_ir::{PlayerIntent, from_ron, to_ron};
use serde::{Deserialize, Serialize};

use crate::error::NetError;

/// Replay payload: hashes plus the PlayerIntent list the host consumed.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReplayFile {
    /// Canon identity at session start.
    pub canon_hash: Hash,
    /// Prefix the client disagreed with (or the host's last prefix).
    pub trace_prefix_hash: Hash,
    /// Intents the host ingested this session, in consume order.
    pub intents: Vec<PlayerIntent>,
}

/// Write a desync replay. The file is RON so [`klotho_ir::from_ron`] can load
/// [`ReplayFile::intents`] as `Vec<PlayerIntent>`.
pub fn write_replay(
    path: &Path,
    canon_hash: Hash,
    trace_prefix_hash: Hash,
    intents: &[PlayerIntent],
) -> Result<(), NetError> {
    let file = ReplayFile {
        canon_hash,
        trace_prefix_hash,
        intents: intents.to_vec(),
    };
    let body = to_ron(&file).map_err(|e| NetError::Replay(e.to_string()))?;
    fs::write(path, body).map_err(|e| NetError::Io(e.to_string()))
}

/// Load a replay written by [`write_replay`].
pub fn load_replay(path: &Path) -> Result<ReplayFile, NetError> {
    let src = fs::read_to_string(path).map_err(|e| NetError::Io(e.to_string()))?;
    from_ron(&src).map_err(|e| NetError::Replay(e.to_string()))
}

/// Intents from a replay file, parsed as `Vec<PlayerIntent>`.
pub fn load_replay_intents(path: &Path) -> Result<Vec<PlayerIntent>, NetError> {
    let file = load_replay(path)?;
    let ron = to_ron(&file.intents).map_err(|e| NetError::Replay(e.to_string()))?;
    from_ron(&ron).map_err(|e| NetError::Replay(e.to_string()))
}

#[cfg(test)]
mod tests {
    use klotho_core::{PlayerId, Tick};
    use klotho_ir::{Agency, Analog, IntentTarget, Verb};

    use super::*;

    fn look() -> PlayerIntent {
        PlayerIntent {
            player: PlayerId(0),
            at: Tick(1),
            verb: Verb::Look,
            target: IntentTarget::None,
            analog: Analog::default(),
            agency: Agency::none(),
        }
    }

    #[test]
    fn replay_round_trip_parses_intents() {
        let path = std::env::temp_dir().join(format!(
            "klotho-net-replay-unit-{}-{}.ron",
            std::process::id(),
            Tick(1).0
        ));
        let intents = vec![look()];
        write_replay(&path, Hash::ZERO, Hash::from_bytes([2; 32]), &intents).unwrap();
        let meta = std::fs::metadata(&path).unwrap();
        assert!(meta.len() > 0);
        let file = load_replay(&path).unwrap();
        assert_eq!(file.intents, intents);
        let parsed = load_replay_intents(&path).unwrap();
        assert_eq!(parsed, intents);
        let _ = std::fs::remove_file(&path);
    }
}
