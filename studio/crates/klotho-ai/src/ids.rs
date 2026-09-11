//! Deterministic 128-bit identifiers. Never RNG.

use core::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use klotho_core::Hash;
use klotho_ir::{AnchorId, Name};
use klotho_prove::hash_bytes;

/// 128-bit change identity, derived from request bytes.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default)]
pub struct ChangeId(pub [u8; 16]);

/// Isolated transaction identity.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default)]
pub struct TxId(pub [u8; 16]);

/// Anchor-tree lease identity.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default)]
pub struct LeaseId(pub [u8; 16]);

/// Stub asset request identity until Weaver lands.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default)]
pub struct AssetRequestId(pub [u8; 16]);

/// Stub reference identity until the reference graph lands.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default)]
pub struct ReferenceId(pub [u8; 16]);

/// Closed addressable field. Not a free-form path.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[repr(u8)]
pub enum FieldId {
    /// Current [`Name`].
    Name = 0,
    /// Locus kind.
    Kind = 1,
    /// Seed fact collection.
    SeedFact = 2,
    /// Quantity row.
    Qty = 3,
    /// Pose row.
    Pose = 4,
    /// Relation row.
    Rel = 5,
    /// Canon patch.
    CanonDiff = 6,
    /// Module body / lock.
    ModuleBody = 7,
    /// Name alias table.
    Alias = 8,
    /// Removal tombstone.
    Tombstone = 9,
    /// Module parameter.
    Parameter = 10,
    /// Pattern argument.
    PatternArg = 11,
    /// Pattern version.
    PatternVersion = 12,
    /// Asset candidate binding.
    AssetBinding = 13,
    /// Journey spec.
    Journey = 14,
    /// Reference edge.
    Reference = 15,
    /// Ordered child collection.
    CollectionOrder = 16,
    /// Import row.
    Import = 17,
}

/// One read or write cell.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cell {
    /// Object identity.
    pub anchor: AnchorId,
    /// Field within that object.
    pub field: FieldId,
}

impl ChangeId {
    /// Truncated blake3 of `klotho-change-v1 || bytes`.
    #[must_use]
    pub fn derive(bytes: &[u8]) -> Self {
        Self(derive16(b"klotho-change-v1", bytes))
    }

    /// Borrow the bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl TxId {
    /// Truncated blake3 of `klotho-tx-v1 || bytes`.
    #[must_use]
    pub fn derive(bytes: &[u8]) -> Self {
        Self(derive16(b"klotho-tx-v1", bytes))
    }

    /// Borrow the bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl LeaseId {
    /// Truncated blake3 of `klotho-lease-v1 || bytes`.
    #[must_use]
    pub fn derive(bytes: &[u8]) -> Self {
        Self(derive16(b"klotho-lease-v1", bytes))
    }
}

impl AssetRequestId {
    /// Truncated blake3 of `klotho-asset-req-v1 || bytes`.
    #[must_use]
    pub fn derive(bytes: &[u8]) -> Self {
        Self(derive16(b"klotho-asset-req-v1", bytes))
    }
}

impl ReferenceId {
    /// Truncated blake3 of `klotho-ref-v1 || bytes`.
    #[must_use]
    pub fn derive(bytes: &[u8]) -> Self {
        Self(derive16(b"klotho-ref-v1", bytes))
    }
}

/// `AnchorId::derive(project, change).child(token)`.
#[must_use]
pub fn derive_op_anchor(project: &Name, change: ChangeId, token: &[u8]) -> AnchorId {
    AnchorId::derive(project.as_str().as_bytes(), change.as_bytes()).child(token)
}

fn derive16(domain: &[u8], bytes: &[u8]) -> [u8; 16] {
    let mut buf = Vec::with_capacity(domain.len() + 4 + bytes.len());
    buf.extend_from_slice(domain);
    buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(bytes);
    let hash: Hash = hash_bytes(&buf);
    let mut out = [0u8; 16];
    out.copy_from_slice(&hash.0[..16]);
    out
}

fn parse_hex16(s: &str) -> Result<[u8; 16], &'static str> {
    if s.len() != 32 {
        return Err("id must be 32 hex characters");
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
        _ => Err("id contains a non-hex character"),
    }
}

macro_rules! hex16_serde {
    ($ty:ident) => {
        impl fmt::Display for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                for b in &self.0 {
                    write!(f, "{b:02x}")?;
                }
                Ok(())
            }
        }

        impl Serialize for $ty {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.to_string())
            }
        }

        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let s = String::deserialize(deserializer)?;
                parse_hex16(&s).map($ty).map_err(serde::de::Error::custom)
            }
        }
    };
}

hex16_serde!(ChangeId);
hex16_serde!(TxId);
hex16_serde!(LeaseId);
hex16_serde!(AssetRequestId);
hex16_serde!(ReferenceId);
