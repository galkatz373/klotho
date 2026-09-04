//! Little-endian save file codec. Caps every length prefix.

use klotho_core::{Epoch, Hash, Tick};
use klotho_trace::{decode_event, encode_event};
use klotho_world::WorldSnapshot;

use crate::blob::SaveBlob;
use crate::error::SaveError;

/// Save file magic.
pub const SAVE_MAGIC: [u8; 4] = *b"KSAV";
/// Save file version.
pub const SAVE_VERSION: u8 = 1;
/// Total encoded size cap.
pub const SAVE_CAP: usize = 64 * 1024 * 1024;
/// Per-event payload cap.
pub const MAX_EVENT_BYTES: usize = 1024 * 1024;
/// Suffix event-count cap.
pub const MAX_SUFFIX_EVENTS: usize = 1_048_576;
/// Bytes through `snap_len` (inclusive).
const SAVE_PREFIX_LEN: usize = 92;

/// Header + snap payload + suffix length field + suffix bytes.
#[must_use]
pub fn assembled_size(snap_len: usize, suffix_bytes: usize) -> usize {
    SAVE_PREFIX_LEN
        .saturating_add(snap_len)
        .saturating_add(4)
        .saturating_add(suffix_bytes)
}

/// Refuse before allocating a blob larger than [`SAVE_CAP`].
pub fn check_assembled_size(size: usize) -> Result<(), SaveError> {
    if size > SAVE_CAP {
        Err(SaveError::Oversize {
            size,
            cap: SAVE_CAP,
        })
    } else {
        Ok(())
    }
}

/// Encode a save blob. Fails closed if the assembled size would exceed [`SAVE_CAP`].
pub fn encode(blob: &SaveBlob) -> Result<Vec<u8>, SaveError> {
    let snap_bytes = blob.snap.encode()?;
    if snap_bytes.len() > SAVE_CAP {
        return Err(SaveError::Oversize {
            size: snap_bytes.len(),
            cap: SAVE_CAP,
        });
    }
    if blob.suffix.len() > MAX_SUFFIX_EVENTS {
        return Err(SaveError::Oversize {
            size: blob.suffix.len(),
            cap: MAX_SUFFIX_EVENTS,
        });
    }
    let mut suffix_bytes = Vec::new();
    for e in &blob.suffix {
        if e.tick <= blob.trace_from_tick {
            return Err(SaveError::TickWindow);
        }
        let ev = encode_event(e);
        if ev.len() > MAX_EVENT_BYTES {
            return Err(SaveError::Oversize {
                size: ev.len(),
                cap: MAX_EVENT_BYTES,
            });
        }
        let n = u32::try_from(ev.len()).map_err(|_| SaveError::Oversize {
            size: ev.len(),
            cap: u32::MAX as usize,
        })?;
        suffix_bytes.extend_from_slice(&n.to_le_bytes());
        suffix_bytes.extend_from_slice(&ev);
        check_assembled_size(assembled_size(snap_bytes.len(), suffix_bytes.len()))?;
    }
    let size = assembled_size(snap_bytes.len(), suffix_bytes.len());
    check_assembled_size(size)?;
    let snap_len = u32::try_from(snap_bytes.len()).map_err(|_| SaveError::Oversize {
        size: snap_bytes.len(),
        cap: u32::MAX as usize,
    })?;
    let suffix_len = u32::try_from(blob.suffix.len()).map_err(|_| SaveError::Oversize {
        size: blob.suffix.len(),
        cap: u32::MAX as usize,
    })?;
    let mut buf = Vec::with_capacity(size);
    buf.extend_from_slice(&SAVE_MAGIC);
    buf.push(SAVE_VERSION);
    buf.extend_from_slice(&[0, 0, 0]);
    buf.extend_from_slice(blob.canon_hash.as_bytes());
    buf.extend_from_slice(&blob.epoch.0.to_le_bytes());
    buf.extend_from_slice(blob.prefix.as_bytes());
    buf.extend_from_slice(&blob.trace_from_tick.0.to_le_bytes());
    buf.extend_from_slice(&snap_len.to_le_bytes());
    buf.extend_from_slice(&snap_bytes);
    buf.extend_from_slice(&suffix_len.to_le_bytes());
    buf.extend_from_slice(&suffix_bytes);
    Ok(buf)
}

/// Decode a save blob. Declared lengths larger than [`SAVE_CAP`] fail before alloc.
pub fn decode(bytes: &[u8]) -> Result<SaveBlob, SaveError> {
    if bytes.len() > SAVE_CAP {
        return Err(SaveError::Oversize {
            size: bytes.len(),
            cap: SAVE_CAP,
        });
    }
    if bytes.len() < SAVE_PREFIX_LEN {
        if bytes.len() >= 4 && bytes[..4] != SAVE_MAGIC {
            return Err(SaveError::Magic);
        }
        return Err(SaveError::Truncated);
    }
    let mut rest = bytes;
    let magic = take(&mut rest, 4)?;
    if magic != SAVE_MAGIC {
        return Err(SaveError::Magic);
    }
    let version = take_u8(&mut rest)?;
    if version != SAVE_VERSION {
        return Err(SaveError::Version(version));
    }
    let pad = take(&mut rest, 3)?;
    if pad != [0, 0, 0] {
        return Err(SaveError::Pad);
    }
    let canon_hash = take_hash(&mut rest)?;
    let epoch = Epoch(take_u64(&mut rest)?);
    let prefix = take_hash(&mut rest)?;
    let tick = Tick(take_u64(&mut rest)?);
    let snap_len = take_u32(&mut rest)? as usize;
    if snap_len > SAVE_CAP {
        return Err(SaveError::Oversize {
            size: snap_len,
            cap: SAVE_CAP,
        });
    }
    if snap_len > rest.len() {
        return Err(SaveError::Truncated);
    }
    let (snap_bytes, after_snap) = rest.split_at(snap_len);
    rest = after_snap;
    let snap = WorldSnapshot::decode(snap_bytes)?;
    if rest.len() < 4 {
        return Err(SaveError::Truncated);
    }
    let suffix_len = take_u32(&mut rest)? as usize;
    if suffix_len > MAX_SUFFIX_EVENTS {
        return Err(SaveError::Oversize {
            size: suffix_len,
            cap: MAX_SUFFIX_EVENTS,
        });
    }
    let mut suffix = Vec::new();
    for _ in 0..suffix_len {
        let n = take_u32(&mut rest)? as usize;
        if n > MAX_EVENT_BYTES {
            return Err(SaveError::Oversize {
                size: n,
                cap: MAX_EVENT_BYTES,
            });
        }
        if n > rest.len() {
            return Err(SaveError::Truncated);
        }
        let ev_bytes = take(&mut rest, n)?;
        let ev = decode_event(ev_bytes).map_err(|_| SaveError::BadEvent)?;
        if ev.tick <= tick {
            return Err(SaveError::TickWindow);
        }
        suffix.push(ev);
    }
    if !rest.is_empty() {
        return Err(SaveError::Trailing);
    }
    if snap.canon_hash != canon_hash
        || snap.epoch != epoch
        || snap.trace_prefix_hash != prefix
        || snap.tick != tick
    {
        return Err(SaveError::PrefixMismatch);
    }
    Ok(SaveBlob {
        canon_hash,
        epoch,
        prefix,
        snap: std::sync::Arc::new(snap),
        suffix,
        trace_from_tick: tick,
    })
}

fn take<'a>(rest: &mut &'a [u8], n: usize) -> Result<&'a [u8], SaveError> {
    if rest.len() < n {
        return Err(SaveError::Truncated);
    }
    let (head, tail) = rest.split_at(n);
    *rest = tail;
    Ok(head)
}

fn take_arr<const N: usize>(rest: &mut &[u8]) -> Result<[u8; N], SaveError> {
    let s = take(rest, N)?;
    let mut a = [0u8; N];
    a.copy_from_slice(s);
    Ok(a)
}

fn take_u8(rest: &mut &[u8]) -> Result<u8, SaveError> {
    Ok(take_arr::<1>(rest)?[0])
}

fn take_u32(rest: &mut &[u8]) -> Result<u32, SaveError> {
    Ok(u32::from_le_bytes(take_arr::<4>(rest)?))
}

fn take_u64(rest: &mut &[u8]) -> Result<u64, SaveError> {
    Ok(u64::from_le_bytes(take_arr::<8>(rest)?))
}

fn take_hash(rest: &mut &[u8]) -> Result<Hash, SaveError> {
    Ok(Hash::from_bytes(take_arr::<32>(rest)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_cap_constant_and_assembled_check() {
        assert_eq!(SAVE_CAP, 64 * 1024 * 1024);
        assert_eq!(
            check_assembled_size(SAVE_CAP + 1),
            Err(SaveError::Oversize {
                size: SAVE_CAP + 1,
                cap: SAVE_CAP,
            })
        );
        assert_eq!(check_assembled_size(SAVE_CAP), Ok(()));
        assert_eq!(assembled_size(0, 0), SAVE_PREFIX_LEN + 4);
    }
}
