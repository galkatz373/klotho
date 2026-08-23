//! 32-byte content hashes and CAS blob ids.
//!
//! The digest algorithm (blake3 of canonical little-endian bytes) lives in
//! `klotho-prove`. This crate only freezes the width so Trace prefix hashes
//! and Canon hashes are one type everywhere.

use core::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// 32-byte digest. Cross-OS hashes need one width; the bytes are opaque here.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default)]
pub struct Hash(pub [u8; 32]);

impl Hash {
    /// All-zero digest. Never a valid Canon hash of a cooked warp.
    pub const ZERO: Self = Self([0; 32]);

    /// Construct from raw bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Borrow the bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// XOR two hashes, bytewise. Used to fold `canon_hash ⊕ tick` for [`crate::Rng`].
    #[must_use]
    pub const fn bitxor(self, other: Self) -> Self {
        let mut out = [0u8; 32];
        let mut i = 0;
        while i < 32 {
            out[i] = self.0[i] ^ other.0[i];
            i += 1;
        }
        Self(out)
    }
}

impl From<[u8; 32]> for Hash {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl Serialize for Hash {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Hash {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        parse_hex32(&s).map(Hash).map_err(serde::de::Error::custom)
    }
}

impl core::ops::BitXor for Hash {
    type Output = Self;
    fn bitxor(self, rhs: Self) -> Self {
        Hash::bitxor(self, rhs)
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in &self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

/// Content-addressed blob id. Same width as [`Hash`]; distinct type so hull
/// bindings cannot be confused with Canon / Trace prefix hashes.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default)]
pub struct BlobId(pub [u8; 32]);

impl BlobId {
    /// All-zero id. Not a cooked hull.
    pub const ZERO: Self = Self([0; 32]);

    /// Construct from raw bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Borrow the bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl From<[u8; 32]> for BlobId {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl fmt::Display for BlobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "blob:")?;
        for b in &self.0[..4] {
            write!(f, "{b:02x}")?;
        }
        write!(f, "…")
    }
}

impl Serialize for BlobId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&Hash(self.0).to_string())
    }
}

impl<'de> Deserialize<'de> for BlobId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        parse_hex32(&s)
            .map(BlobId)
            .map_err(serde::de::Error::custom)
    }
}

pub(crate) fn parse_hex32(s: &str) -> Result<[u8; 32], &'static str> {
    if s.len() != 64 {
        return Err("digest must be 64 hex characters");
    }
    let bytes = s.as_bytes();
    let mut out = [0u8; 32];
    let mut i = 0;
    while i < 32 {
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
        _ => Err("digest contains a non-hex character"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xor_is_bytewise() {
        let mut a = [0u8; 32];
        a[0] = 0xff;
        a[31] = 0x0f;
        let mut b = [0u8; 32];
        b[0] = 0x0f;
        b[31] = 0xf0;
        let x = Hash(a).bitxor(Hash(b));
        assert_eq!(x.0[0], 0xf0);
        assert_eq!(x.0[31], 0xff);
        assert_eq!(x.0[1], 0);
    }

    #[test]
    fn display_is_64_hex_chars() {
        assert_eq!(Hash::ZERO.to_string().len(), 64);
        assert_ne!(format!("{}", BlobId::ZERO), format!("{}", Hash::ZERO));
    }

    #[test]
    fn hex_round_trip() {
        let mut bytes = [0u8; 32];
        bytes[0] = 0xab;
        bytes[31] = 0xcd;
        let h = Hash(bytes);
        assert_eq!(parse_hex32(&h.to_string()).unwrap(), bytes);
        assert!(parse_hex32("zz").is_err());
        assert!(parse_hex32(&"0".repeat(63)).is_err());
    }
}
