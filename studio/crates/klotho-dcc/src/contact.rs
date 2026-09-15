//! Approved semantic socket sidecars, separate from visual glTF bone palettes.
use crate::GltfImport;
use klotho_compile::{CompileError, DccArtifact, MAX_CONTACT_TRACK_BYTES, encode_contact_track};
use klotho_core::ContactTrack;

/// Attach an approved quantized semantic export to a visual import.
/// Sidecars sample root/socket/capsule positions at authoritative tick boundaries;
/// `cook_with_dcc` certifies them against the separately authored Canon binding.
/// A visual importer never derives or approves canonical contact geometry.
pub fn with_contact_sidecar(import: GltfImport, bytes: &[u8]) -> Result<DccArtifact, CompileError> {
    if bytes.len() > MAX_CONTACT_TRACK_BYTES {
        return Err(CompileError::Gltf("contact sidecar exceeds cap".into()));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| CompileError::Gltf("contact sidecar is not UTF-8".into()))?;
    let track: ContactTrack = ron::from_str(text).map_err(|e| CompileError::Gltf(e.to_string()))?;
    encode_contact_track(&track)?;
    let mut artifact: DccArtifact = import.into();
    if artifact.clips.is_none() {
        return Err(CompileError::Gltf(
            "contact sidecar requires an animated import".into(),
        ));
    }
    let mut identity = artifact.source_hash.as_bytes().to_vec();
    identity.extend_from_slice(bytes);
    artifact.source_hash = klotho_prove::hash_bytes(&identity);
    artifact.contact_track = Some(track);
    Ok(artifact)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_animated_import_accepts_only_bounded_quantized_sidecar() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/dcc/walk.gltf");
        let imports = crate::import_gltf(&path).unwrap();
        let input = imports.into_iter().find(|i| i.clips.is_some()).unwrap();
        let sidecar =
            include_bytes!("../../../../engine/crates/klotho-compile/fixtures/sword-contact.ron");
        let attached = with_contact_sidecar(input.clone(), sidecar).unwrap();
        assert!(attached.contact_track.is_some());
        assert_ne!(attached.source_hash, input.source_hash);
        assert!(with_contact_sidecar(input.clone(), b"(roots:[NaN])").is_err());
        assert!(with_contact_sidecar(input, &vec![0; MAX_CONTACT_TRACK_BYTES + 1]).is_err());
    }
}
