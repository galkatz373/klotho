//! 32-byte content hashes and CAS blob ids.
//!
//! The digest algorithm (blake3 of canonical little-endian bytes) lives in
//! `klotho-prove`. This crate only freezes the width so Trace prefix hashes
//! and Canon hashes are one type everywhere.

use core::fmt;

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
}
