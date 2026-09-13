//! Production presentation contracts (KAI-17).
//!
//! Quality tiers, bounded material graphs, and casting/consent records are
//! authoring inputs. They compile to Manifest permutations and never execute
//! as gameplay graphs.

use serde::{Deserialize, Serialize};

use klotho_core::Hash;

use crate::error::IrError;
use crate::name::Name;

/// Desktop quality ladder. Fallback is High → Medium → Low and is deterministic.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityTier {
    /// Unlit family, smallest residency.
    Low = 0,
    /// Forward+ without SSGI; one cascade.
    Medium = 1,
    /// 1080p High adventure: probes, SSGI, three cascades.
    High = 2,
}

impl QualityTier {
    /// Catalog name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    /// Packed discriminant.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// Decode a packed tier.
    #[must_use]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Low),
            1 => Some(Self::Medium),
            2 => Some(Self::High),
            _ => None,
        }
    }

    /// Next cheaper tier, or `None` at Low.
    #[must_use]
    pub const fn fallback(self) -> Option<Self> {
        match self {
            Self::High => Some(Self::Medium),
            Self::Medium => Some(Self::Low),
            Self::Low => None,
        }
    }
}

/// Pinned 1080p presentation caps for one quality tier.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresentProfile {
    /// Quality ladder step.
    pub tier: QualityTier,
    /// Present wall time, microseconds. High is 11_000.
    pub us_present: u32,
    /// Resident texture+geometry budget, mebibytes.
    pub vram_mb: u16,
    /// Cluster draw cap after drop-farthest.
    pub max_clusters: u16,
    /// GPU particle emitter cap.
    pub max_particles: u16,
    /// Ribbon strip cap.
    pub max_ribbons: u16,
    /// Skinned instance cap.
    pub max_skinned: u16,
}

impl PresentProfile {
    /// 1080p High adventure. Tapestry stress must meet this.
    #[must_use]
    pub const fn high() -> Self {
        Self {
            tier: QualityTier::High,
            us_present: 11_000,
            vram_mb: 1_536,
            max_clusters: 2_048,
            max_particles: 1_024,
            max_ribbons: 128,
            max_skinned: 256,
        }
    }

    /// Medium fallback: no SSGI, one cascade, half particles.
    #[must_use]
    pub const fn medium() -> Self {
        Self {
            tier: QualityTier::Medium,
            us_present: 11_000,
            vram_mb: 1_024,
            max_clusters: 1_024,
            max_particles: 512,
            max_ribbons: 64,
            max_skinned: 128,
        }
    }

    /// Low fallback: unlit family.
    #[must_use]
    pub const fn low() -> Self {
        Self {
            tier: QualityTier::Low,
            us_present: 11_000,
            vram_mb: 512,
            max_clusters: 512,
            max_particles: 128,
            max_ribbons: 16,
            max_skinned: 64,
        }
    }

    /// Profile for `tier`.
    #[must_use]
    pub const fn for_tier(tier: QualityTier) -> Self {
        match tier {
            QualityTier::High => Self::high(),
            QualityTier::Medium => Self::medium(),
            QualityTier::Low => Self::low(),
        }
    }

    /// Fail closed on inverted caps.
    pub fn validate(&self) -> Result<(), IrError> {
        if self.us_present == 0 {
            return Err(invalid("us_present", "must be > 0"));
        }
        if self.vram_mb == 0 {
            return Err(invalid("vram_mb", "must be > 0"));
        }
        if PresentProfile::for_tier(self.tier) != *self {
            return Err(invalid(
                "tier",
                "profile fields must match the pinned tier table",
            ));
        }
        Ok(())
    }
}

fn invalid(field: &str, reason: &str) -> IrError {
    IrError::InvalidPresent {
        field: field.into(),
        reason: reason.into(),
    }
}

/// Closed material-graph node set. Cook compiles this; it is not a gameplay graph.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialNode {
    /// Constant RGB, milli-units (`0..=1000`).
    Constant {
        /// Milli-RGB.
        rgb_milli: [u16; 3],
    },
    /// Sample texture slot `0..=7`.
    Sample {
        /// Texture slot.
        slot: u8,
    },
    /// `nodes[a] * nodes[b]`.
    Mul {
        /// Left index.
        a: u8,
        /// Right index.
        b: u8,
    },
    /// Mix `a`/`b` by `t`.
    Lerp {
        /// From.
        a: u8,
        /// To.
        b: u8,
        /// Weight.
        t: u8,
    },
    /// Clamp `x` to milli-range.
    Clamp {
        /// Source index.
        x: u8,
        /// Inclusive lower milli.
        lo_milli: u16,
        /// Inclusive upper milli.
        hi_milli: u16,
    },
}

/// Bounded authored material. Compiles to a shader permutation bitset.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialGraph {
    /// Stable authoring id.
    pub id: Name,
    /// Nodes in evaluation order. Cap is [`Self::MAX_NODES`].
    pub nodes: Vec<MaterialNode>,
    /// Albedo node index.
    pub albedo: u8,
    /// Metalness node index.
    pub metalness: u8,
    /// Roughness node index.
    pub roughness: u8,
    /// Optional emissive node.
    pub emissive: Option<u8>,
    /// Optional alpha node (masked pass).
    pub alpha: Option<u8>,
}

impl MaterialGraph {
    /// Hard node cap. Over-cap fails closed.
    pub const MAX_NODES: usize = 16;
    /// Texture slots `0..=7`.
    pub const MAX_SLOTS: u8 = 8;

    /// Organic constant albedo, mid metal/rough.
    #[must_use]
    pub fn organic() -> Self {
        Self {
            id: Name::from("organic"),
            nodes: vec![
                MaterialNode::Constant {
                    rgb_milli: [620, 480, 310],
                },
                MaterialNode::Constant {
                    rgb_milli: [50, 50, 50],
                },
                MaterialNode::Constant {
                    rgb_milli: [700, 700, 700],
                },
            ],
            albedo: 0,
            metalness: 1,
            roughness: 2,
            emissive: None,
            alpha: None,
        }
    }

    /// Fail closed on empty id, over-cap, or out-of-range indices.
    pub fn validate(&self) -> Result<(), IrError> {
        self.id.check()?;
        if self.nodes.is_empty() {
            return Err(invalid("nodes", "empty graph"));
        }
        if self.nodes.len() > Self::MAX_NODES {
            return Err(invalid("nodes", "exceeds MAX_NODES"));
        }
        let n = u8::try_from(self.nodes.len()).unwrap_or(u8::MAX);
        for (i, node) in self.nodes.iter().enumerate() {
            match *node {
                MaterialNode::Constant { rgb_milli } => {
                    if rgb_milli.iter().any(|&c| c > 1_000) {
                        return Err(invalid("constant", "rgb_milli > 1000"));
                    }
                }
                MaterialNode::Sample { slot } => {
                    if slot >= Self::MAX_SLOTS {
                        return Err(invalid("sample", "slot >= 8"));
                    }
                }
                MaterialNode::Mul { a, b } => {
                    check_ref("mul", a, n, i)?;
                    check_ref("mul", b, n, i)?;
                }
                MaterialNode::Lerp { a, b, t } => {
                    check_ref("lerp", a, n, i)?;
                    check_ref("lerp", b, n, i)?;
                    check_ref("lerp", t, n, i)?;
                }
                MaterialNode::Clamp {
                    x,
                    lo_milli,
                    hi_milli,
                } => {
                    check_ref("clamp", x, n, i)?;
                    if lo_milli > 1_000 || hi_milli > 1_000 || lo_milli > hi_milli {
                        return Err(invalid("clamp", "milli range"));
                    }
                }
            }
        }
        check_out("albedo", self.albedo, n)?;
        check_out("metalness", self.metalness, n)?;
        check_out("roughness", self.roughness, n)?;
        if let Some(e) = self.emissive {
            check_out("emissive", e, n)?;
        }
        if let Some(a) = self.alpha {
            check_out("alpha", a, n)?;
        }
        Ok(())
    }
}

fn check_ref(field: &str, idx: u8, n: u8, at: usize) -> Result<(), IrError> {
    if idx >= n || usize::from(idx) >= at {
        return Err(invalid(field, "forward or oob node ref"));
    }
    Ok(())
}

fn check_out(field: &str, idx: u8, n: u8) -> Result<(), IrError> {
    if idx >= n {
        return Err(invalid(field, "output index oob"));
    }
    Ok(())
}

/// Performer/casting record required before generated or recorded VO/face ships.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CastingConsent {
    /// Performer or session identity.
    pub performer: Name,
    /// Role or line group.
    pub role: Name,
    /// Consent document hash. Zero is refused.
    pub consent: Hash,
    /// Union/territory tag (`sag-aftra-us`, `non-union-dev`).
    pub union_territory: Name,
    /// Whether reuse beyond this title is granted.
    pub reuse: bool,
    /// Named audio/legal approver.
    pub approved_by: Name,
}

impl CastingConsent {
    /// First-title fixture: named human approval, non-zero consent.
    #[must_use]
    pub fn first_title() -> Self {
        Self {
            performer: Name::from("fixture-performer"),
            role: Name::from("hero"),
            consent: Hash::from_bytes([0x11; 32]),
            union_territory: Name::from("non-union-dev"),
            reuse: false,
            approved_by: Name::from("audio-lead"),
        }
    }

    /// Generated VO/music may not ship without a complete record.
    pub fn validate(&self) -> Result<(), IrError> {
        self.performer.check()?;
        self.role.check()?;
        self.union_territory.check()?;
        self.approved_by.check()?;
        if self.consent == Hash::ZERO {
            return Err(invalid("consent", "zero hash"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_are_stable_and_fall_back() {
        assert_eq!(QualityTier::High.as_u8(), 2);
        assert_eq!(QualityTier::from_u8(1), Some(QualityTier::Medium));
        assert_eq!(QualityTier::from_u8(3), None);
        assert_eq!(QualityTier::High.fallback(), Some(QualityTier::Medium));
        assert_eq!(QualityTier::Medium.fallback(), Some(QualityTier::Low));
        assert_eq!(QualityTier::Low.fallback(), None);
        PresentProfile::high().validate().unwrap();
        PresentProfile::medium().validate().unwrap();
        PresentProfile::low().validate().unwrap();
    }

    #[test]
    fn drifted_profile_fails() {
        let mut p = PresentProfile::high();
        p.vram_mb = 1;
        assert!(p.validate().is_err());
    }

    #[test]
    fn organic_graph_validates_and_overcap_fails() {
        let g = MaterialGraph::organic();
        g.validate().unwrap();
        let mut bad = g.clone();
        bad.nodes = vec![MaterialNode::Constant { rgb_milli: [0; 3] }; 17];
        assert!(bad.validate().is_err());
        let mut fwd = MaterialGraph::organic();
        fwd.nodes[0] = MaterialNode::Mul { a: 1, b: 0 };
        assert!(fwd.validate().is_err());
    }

    #[test]
    fn consent_zero_hash_fails() {
        CastingConsent::first_title().validate().unwrap();
        let mut c = CastingConsent::first_title();
        c.consent = Hash::ZERO;
        assert!(c.validate().is_err());
    }
}
