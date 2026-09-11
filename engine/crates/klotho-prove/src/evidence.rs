//! Content-addressed evidence signatures. Not public-key signatures.

use klotho_core::Hash;

use crate::digest::hash_bytes;

/// Domain mixed into every evidence seal. Bump ⇒ every signature changes.
const EVIDENCE_DOMAIN: &[u8] = b"klotho-evidence-v1";

/// blake3 of `klotho-evidence-v1 || le32(len) || payload`.
#[must_use]
pub fn evidence_signature(payload: &[u8]) -> Hash {
    let mut buf = Vec::with_capacity(EVIDENCE_DOMAIN.len() + 4 + payload.len());
    buf.extend_from_slice(EVIDENCE_DOMAIN);
    buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    buf.extend_from_slice(payload);
    hash_bytes(&buf)
}

/// `true` when `signature` is the seal of `payload`.
#[must_use]
pub fn evidence_matches(payload: &[u8], signature: Hash) -> bool {
    evidence_signature(payload) == signature
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_payload_same_signature() {
        let a = evidence_signature(b"bundle");
        let b = evidence_signature(b"bundle");
        assert_eq!(a, b);
        assert!(evidence_matches(b"bundle", a));
        assert!(!evidence_matches(b"other", a));
        assert_ne!(a, Hash::ZERO);
    }

    #[test]
    fn domain_is_not_raw_blake3() {
        assert_ne!(evidence_signature(b"bundle"), hash_bytes(b"bundle"));
    }
}
