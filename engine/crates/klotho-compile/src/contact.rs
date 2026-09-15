//! Bounded semantic contact cook and visual retarget compatibility.
use crate::{
    CompileError,
    header::{PREFIX, peek_kind, write_prefix},
};
use klotho_core::{CONTACT_ERROR_MM, ContactTrack, Hash, IVec3};
use klotho_prove::{ArtifactKind, hash_bytes};

/// Maximum encoded semantic track size; checked before parsing untrusted bytes.
pub const MAX_CONTACT_TRACK_BYTES: usize = 128 * 1024;
fn invalid() -> CompileError {
    CompileError::Header("invalid semantic contact track".into())
}
/// Cook already-quantized authoritative tick samples. No render interpolation is consumed.
pub fn encode_contact_track(track: &ContactTrack) -> Result<Vec<u8>, CompileError> {
    if !track.is_valid() {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    write_prefix(&mut bytes, ArtifactKind::ContactTrack);
    bytes.extend_from_slice(ron::to_string(track).map_err(|_| invalid())?.as_bytes());
    if bytes.len() > MAX_CONTACT_TRACK_BYTES {
        return Err(invalid());
    }
    Ok(bytes)
}
/// Validate prefix, bounds, payload and canonical encoding before exposing samples.
pub fn decode_contact_track(bytes: &[u8]) -> Result<ContactTrack, CompileError> {
    if bytes.len() > MAX_CONTACT_TRACK_BYTES || peek_kind(bytes)? != ArtifactKind::ContactTrack {
        return Err(invalid());
    }
    let text = std::str::from_utf8(&bytes[PREFIX..]).map_err(|_| invalid())?;
    let track: ContactTrack = ron::from_str(text).map_err(|_| invalid())?;
    if encode_contact_track(&track)? != bytes {
        return Err(invalid());
    }
    Ok(track)
}
/// Compatibility signature binds all semantic geometry, identities and Rite timing.
pub fn contact_signature(track: &ContactTrack) -> Result<Hash, CompileError> {
    Ok(hash_bytes(&encode_contact_track(track)?))
}
fn near(a: &IVec3, b: &IVec3) -> bool {
    let d = [
        i64::from(a.x) - i64::from(b.x),
        i64::from(a.y) - i64::from(b.y),
        i64::from(a.z) - i64::from(b.z),
    ];
    d.iter().map(|v| v * v).sum::<i64>() <= i64::from(CONTACT_ERROR_MM).pow(2)
}
/// Certify a visual clip's quantized semantic samples against the Canon track.
/// Cosmetic palettes and clip bytes are not inputs. Root/timing/rig changes fail closed;
/// retargeted sockets and capsule endpoints must stay within the 5 mm envelope.
pub fn certify_contact_clip(
    canon: &ContactTrack,
    visual: &ContactTrack,
    expected: Hash,
) -> Result<Hash, CompileError> {
    let signature = contact_signature(canon)?;
    if signature != expected || !visual.is_valid() {
        return Err(invalid());
    }
    let mut normalized = visual.clone();
    for (a, b) in canon.sockets.iter().zip(&visual.sockets) {
        if a.name != b.name || !a.samples.iter().zip(&b.samples).all(|(a, b)| near(a, b)) {
            return Err(invalid());
        }
    }
    for (a, b) in canon.sweeps.iter().zip(&visual.sweeps) {
        if a.name != b.name
            || a.socket != b.socket
            || a.radius_mm != b.radius_mm
            || !a
                .samples
                .iter()
                .flatten()
                .zip(b.samples.iter().flatten())
                .all(|(a, b)| near(a, b))
        {
            return Err(invalid());
        }
    }
    if canon.sockets.len() != visual.sockets.len() || canon.sweeps.len() != visual.sweeps.len() {
        return Err(invalid());
    }
    normalized.sockets = canon.sockets.clone();
    normalized.sweeps = canon.sweeps.clone();
    if normalized != *canon {
        return Err(invalid());
    }
    Ok(signature)
}
