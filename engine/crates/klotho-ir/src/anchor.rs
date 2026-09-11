//! Immutable 128-bit authoring identity. Independent of names.

use core::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use klotho_core::Hash;
use klotho_prove::hash_bytes;

/// 128-bit authoring identity.
///
/// Derived from a project namespace and an operation-local token (K81). Never
/// from a mutable name, list position, packed runtime index, or RNG. Rename
/// changes [`crate::Name`] only.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default)]
pub struct AnchorId(pub [u8; 16]);

const DOMAIN: &[u8] = b"klotho-anchor-v1";

impl AnchorId {
    /// All-zero id. Never assigned by [`Self::derive`].
    pub const ZERO: Self = Self([0; 16]);

    /// Construct from raw bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Borrow the bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// Deterministic identity: `blake3(domain || ns || token)` truncated to 16 bytes.
    #[must_use]
    pub fn derive(namespace: &[u8], token: &[u8]) -> Self {
        let mut buf = Vec::with_capacity(DOMAIN.len() + 8 + namespace.len() + token.len());
        buf.extend_from_slice(DOMAIN);
        buf.extend_from_slice(&(namespace.len() as u32).to_le_bytes());
        buf.extend_from_slice(namespace);
        buf.extend_from_slice(&(token.len() as u32).to_le_bytes());
        buf.extend_from_slice(token);
        let hash: Hash = hash_bytes(&buf);
        let mut out = [0u8; 16];
        out.copy_from_slice(&hash.0[..16]);
        Self(out)
    }

    /// Child identity from `(parent, local-id)`. Pattern expansion uses this.
    #[must_use]
    pub fn child(self, local_id: &[u8]) -> Self {
        Self::derive(&self.0, local_id)
    }
}

impl fmt::Display for AnchorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in &self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

impl Serialize for AnchorId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for AnchorId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        parse_hex16(&s)
            .map(AnchorId)
            .map_err(serde::de::Error::custom)
    }
}

fn parse_hex16(s: &str) -> Result<[u8; 16], &'static str> {
    if s.len() != 32 {
        return Err("anchor must be 32 hex characters");
    }
    let bytes = s.as_bytes();
    let mut out = [0u8; 16];
    let mut i = 0;
    while i < 16 {
        let hi = hex_val(bytes[i * 2])?;
        let lo = hex_val(bytes[i * 2 + 1])?;
        out[i] = (hi << 4) | lo;
        i += 1;
    }
    Ok(out)
}

const fn hex_val(b: u8) -> Result<u8, &'static str> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err("anchor contains a non-hex character"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::from_ron;
    use crate::parse::to_ron;

    #[test]
    fn derive_is_stable_and_not_zero() {
        let a = AnchorId::derive(b"hearth", b"module:main");
        let b = AnchorId::derive(b"hearth", b"module:main");
        assert_eq!(a, b);
        assert_ne!(a, AnchorId::ZERO);
        assert_ne!(a, AnchorId::derive(b"hearth", b"module:other"));
    }

    #[test]
    fn child_depends_on_parent_not_on_sibling_order() {
        let parent = AnchorId::derive(b"proj", b"module:a");
        let locus = parent.child(b"locus:oak_door");
        let again = parent.child(b"locus:oak_door");
        assert_eq!(locus, again);
        assert_ne!(locus, parent.child(b"locus:barrel"));
        assert_ne!(locus, parent);
    }

    #[test]
    fn ron_round_trip() {
        let a = AnchorId::derive(b"ns", b"token");
        let text = to_ron(&a).unwrap();
        let b: AnchorId = from_ron(&text).unwrap();
        assert_eq!(a, b);
        assert_eq!(text.trim().len(), 34); // quoted 32 hex chars
    }

    #[test]
    fn bad_hex_is_rejected() {
        assert!(from_ron::<AnchorId>("\"zz\"").is_err());
        assert!(from_ron::<AnchorId>("\"0123\"").is_err());
    }
}
