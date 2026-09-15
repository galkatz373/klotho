//! Disposable read-only semantic motion preview at authoritative boundaries.
use klotho_compile::{CompileError, contact_signature};
use klotho_core::{ContactTrack, Hash, IVec3};

/// One capsule overlay. Render interpolation cannot feed this back into gameplay.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct SweepOverlay {
    /// Semantic channel name.
    pub channel: String,
    /// Capsule endpoints at this boundary and the next.
    pub endpoints: [[IVec3; 2]; 2],
    /// Millimetre radius.
    pub radius_mm: i32,
}
/// Canon contact preview, with root-local socket and sweep geometry.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ContactPreview {
    /// Canon compatibility identity.
    pub signature: Hash,
    /// Authoritative boundary, never presentation time.
    pub tick: u16,
    /// Desired root position.
    pub root: IVec3,
    /// Named semantic sockets.
    pub sockets: Vec<(String, IVec3)>,
    /// Swept capsules for this authoritative interval.
    pub sweeps: Vec<SweepOverlay>,
    /// Planted semantic feet.
    pub planted: Vec<String>,
    /// Whether this interval is in the cooked WAIT window.
    pub active: bool,
}
/// Build an overlay without modifying Canon, Projection, Intent or Trace.
pub fn preview_contact_track(
    track: &ContactTrack,
    tick: u16,
) -> Result<ContactPreview, CompileError> {
    let signature = contact_signature(track)?;
    if tick > track.wait_ticks {
        return Err(CompileError::Header(
            "contact preview boundary out of range".into(),
        ));
    }
    let i = usize::from(tick);
    let next = (i + 1).min(track.roots.len() - 1);
    Ok(ContactPreview {
        signature,
        tick,
        root: track.roots[i],
        active: tick < track.wait_ticks,
        sockets: track
            .sockets
            .iter()
            .map(|s| (s.name.clone(), s.samples[i]))
            .collect(),
        sweeps: track
            .sweeps
            .iter()
            .map(|s| SweepOverlay {
                channel: s.name.clone(),
                endpoints: [s.samples[i], s.samples[next]],
                radius_mm: s.radius_mm,
            })
            .collect(),
        planted: track
            .plants
            .iter()
            .filter(|p| p.start <= tick && tick < p.end)
            .map(|p| p.socket.clone())
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_is_bounded_and_disposable() {
        let track: ContactTrack = klotho_ir::from_ron(include_str!(
            "../../../../engine/crates/klotho-compile/fixtures/sword-contact.ron"
        ))
        .unwrap();
        let before = track.clone();
        let a = preview_contact_track(&track, 0).unwrap();
        let b = preview_contact_track(&track, 1).unwrap();
        assert_ne!(a.sweeps, b.sweeps);
        assert!(a.active);
        assert!(!preview_contact_track(&track, 2).unwrap().active);
        assert!(preview_contact_track(&track, 3).is_err());
        assert_eq!(track, before);
        assert_eq!(a.signature, b.signature);
    }
}
