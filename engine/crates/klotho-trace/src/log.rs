//! Append-only Trace and its prefix hash.

use klotho_core::Hash;
use klotho_prove::hash_bytes;

use crate::encode::encode_event;
use crate::error::TraceError;
use crate::event::TraceEvent;

/// Domain-separated empty prefix. Not [`Hash::ZERO`].
pub const GENESIS_DOMAIN: &[u8] = b"klotho-trace/v1";

/// blake3 of [`GENESIS_DOMAIN`]. Empty log starts here.
#[must_use]
pub fn genesis_hash() -> Hash {
    hash_bytes(GENESIS_DOMAIN)
}

/// Fold `prefix = H(prefix || encode(event))` over `events`.
#[must_use]
pub fn fold_prefix(start: Hash, events: &[TraceEvent]) -> Hash {
    let mut prefix = start;
    for e in events {
        prefix = mix(prefix, e);
    }
    prefix
}

fn mix(prefix: Hash, e: &TraceEvent) -> Hash {
    let enc = encode_event(e);
    let mut buf = Vec::with_capacity(32 + enc.len());
    buf.extend_from_slice(prefix.as_bytes());
    buf.extend_from_slice(&enc);
    hash_bytes(&buf)
}

/// Append-only admitted history. Replay = same events ⇒ same prefix hash.
#[derive(Clone, Debug)]
pub struct TraceLog {
    events: Vec<TraceEvent>,
    prefix: Hash,
}

impl Default for TraceLog {
    fn default() -> Self {
        Self::new()
    }
}

impl TraceLog {
    /// Empty log at genesis.
    #[must_use]
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            prefix: genesis_hash(),
        }
    }

    /// Rebuild from an event list. Prefix is folded from genesis.
    #[must_use]
    pub fn from_events(events: Vec<TraceEvent>) -> Self {
        let prefix = fold_prefix(genesis_hash(), &events);
        Self { events, prefix }
    }

    /// Append an admitted event. Returns the new prefix.
    pub fn append(&mut self, e: TraceEvent) -> Hash {
        self.prefix = mix(self.prefix, &e);
        self.events.push(e);
        self.prefix
    }

    /// Current prefix hash (K19 ancestry).
    #[must_use]
    pub fn prefix_hash(&self) -> Hash {
        self.prefix
    }

    /// Admitted events, oldest first.
    #[must_use]
    pub fn events(&self) -> &[TraceEvent] {
        &self.events
    }

    /// Event count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// No events.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Prefix after the first `n` events. `n == 0` is genesis.
    #[must_use]
    pub fn prefix_at(&self, n: usize) -> Hash {
        fold_prefix(genesis_hash(), &self.events[..n.min(self.events.len())])
    }

    /// Replay `suffix` onto `start`. The first suffix event must mix to the
    /// next hash after `start` (save/load K19).
    #[must_use]
    pub fn replay_suffix(start: Hash, suffix: &[TraceEvent]) -> Hash {
        fold_prefix(start, suffix)
    }

    /// Encode/decode round-trip of every event, then compare prefixes.
    pub fn replay_eq(&self) -> Result<bool, TraceError> {
        let mut copy = TraceLog::new();
        for e in &self.events {
            let bytes = encode_event(e);
            let back = crate::encode::decode_event(&bytes)?;
            if back != *e {
                return Ok(false);
            }
            copy.append(back);
        }
        Ok(copy.prefix == self.prefix && copy.events == self.events)
    }
}

impl PartialEq for TraceLog {
    fn eq(&self, other: &Self) -> bool {
        self.prefix == other.prefix && self.events == other.events
    }
}

impl Eq for TraceLog {}

#[cfg(test)]
mod tests {
    use klotho_core::{LocusKind, ResourceId, Sigil, Tick};

    use super::*;
    use crate::event::{TraceBody, TraceEvent};

    fn actor(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Actor, 0, id).unwrap()
    }

    fn qty(tick: u64, to: i32) -> TraceEvent {
        TraceEvent::new(
            Tick(tick),
            TraceBody::QtyChanged {
                id: actor(1),
                res: ResourceId(0),
                to,
                quantum: 10,
            },
        )
    }

    #[test]
    fn empty_is_genesis_not_zero() {
        let log = TraceLog::new();
        assert_eq!(log.prefix_hash(), genesis_hash());
        assert_ne!(log.prefix_hash(), Hash::ZERO);
        assert!(log.is_empty());
    }

    #[test]
    fn append_changes_prefix() {
        let mut log = TraceLog::new();
        let before = log.prefix_hash();
        log.append(qty(1, 10));
        assert_ne!(log.prefix_hash(), before);
        log.append(qty(2, 20));
        assert_ne!(log.prefix_hash(), before);
    }

    #[test]
    fn replay_equals_incremental() {
        let events = vec![
            qty(1, 10),
            qty(2, 20),
            TraceEvent::new(Tick(3), TraceBody::SaveRequested),
        ];
        let mut inc = TraceLog::new();
        for e in &events {
            inc.append(e.clone());
        }
        let rebuilt = TraceLog::from_events(events);
        assert_eq!(inc, rebuilt);
        assert!(inc.replay_eq().unwrap());
    }

    #[test]
    fn order_changes_hash() {
        let a = vec![qty(1, 1), qty(2, 2)];
        let b = vec![qty(2, 2), qty(1, 1)];
        assert_ne!(
            TraceLog::from_events(a).prefix_hash(),
            TraceLog::from_events(b).prefix_hash()
        );
    }

    #[test]
    fn suffix_from_checkpoint() {
        let mut log = TraceLog::new();
        log.append(qty(1, 1));
        let mid = log.prefix_hash();
        log.append(qty(2, 2));
        log.append(qty(3, 3));
        let suffix = &log.events()[1..];
        assert_eq!(TraceLog::replay_suffix(mid, suffix), log.prefix_hash());
        assert_eq!(log.prefix_at(1), mid);
    }

    #[test]
    fn golden_genesis_and_one_event() {
        assert_eq!(
            genesis_hash().to_string(),
            "ef09399406d05a96bc78fba7bca295962ea318e2e7afebe672f2806c57cbfac5"
        );
        let mut log = TraceLog::new();
        log.append(qty(1, 10));
        assert_eq!(
            log.prefix_hash().to_string(),
            "8a7395d9f708523ceaad9c2fbe9872c527ef36e14f9534131be4daa6ef3845ca"
        );
    }
}
