//! Stable identity: [`Sigil`] naming a [`LocusKind`], plus packed small ids.

use core::fmt;

/// Kind byte packed into the top 8 bits of a [`Sigil`].
///
/// Discriminant 0 is reserved (invalid). Generation wrap (256) is v1-accepted:
/// a Sigil is never reused within a Trace prefix that still references it.
#[repr(u8)]
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub enum LocusKind {
    /// Player or NPC body.
    Actor = 1,
    /// Addressable region of the world (a room, an island).
    Place = 2,
    /// Portable / inspectable object.
    Relic = 3,
    /// A cooked Law (rarely addressable as a locus; reserved).
    Law = 4,
    /// Episode / encounter state chart.
    Beat = 5,
    /// Multi-actor chorus / faction.
    Chorus = 6,
    /// Camera / observer (Hearth eye).
    Observer = 7,
}

impl LocusKind {
    /// Decode a packed kind byte. `None` for 0 or unknown future values.
    #[must_use]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::Actor),
            2 => Some(Self::Place),
            3 => Some(Self::Relic),
            4 => Some(Self::Law),
            5 => Some(Self::Beat),
            6 => Some(Self::Chorus),
            7 => Some(Self::Observer),
            _ => None,
        }
    }

    /// Packed discriminant.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Stable typed id (`u128`).
///
/// Layout, high to low: `8 bit kind | 8 bit generation | 112 bit id-space`.
/// Cook allocates densely; runtime relic churn in Hearth is tens, not millions.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct Sigil(u128);

impl Sigil {
    /// Bit shift of the kind byte.
    pub const KIND_SHIFT: u32 = 120;
    /// Bit shift of the generation byte.
    pub const GEN_SHIFT: u32 = 112;
    /// Width of the dense id-space.
    pub const ID_BITS: u32 = 112;
    /// Mask for the dense id-space.
    pub const ID_MASK: u128 = (1u128 << 112) - 1;

    /// Pack a kind, generation, and 112-bit id. `None` if `id` does not fit.
    #[must_use]
    pub const fn pack(kind: LocusKind, generation: u8, id: u128) -> Option<Self> {
        if id > Self::ID_MASK {
            return None;
        }
        Some(Self::pack_truncated(kind, generation, id))
    }

    /// Pack, masking `id` into 112 bits.
    #[must_use]
    pub const fn pack_truncated(kind: LocusKind, generation: u8, id: u128) -> Self {
        let raw = ((kind as u8 as u128) << Self::KIND_SHIFT)
            | ((generation as u128) << Self::GEN_SHIFT)
            | (id & Self::ID_MASK);
        Self(raw)
    }

    /// Reconstruct from a raw `u128`. Does not validate the kind byte.
    #[must_use]
    pub const fn from_raw(raw: u128) -> Self {
        Self(raw)
    }

    /// Raw bits.
    #[must_use]
    pub const fn raw(self) -> u128 {
        self.0
    }

    /// Kind byte, if it is a known [`LocusKind`].
    #[must_use]
    pub const fn kind(self) -> Option<LocusKind> {
        LocusKind::from_u8(((self.0 >> Self::KIND_SHIFT) & 0xff) as u8)
    }

    /// Generation byte.
    #[must_use]
    pub const fn generation(self) -> u8 {
        ((self.0 >> Self::GEN_SHIFT) & 0xff) as u8
    }

    /// Dense 112-bit id.
    #[must_use]
    pub const fn id(self) -> u128 {
        self.0 & Self::ID_MASK
    }
}

impl fmt::Display for Sigil {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind() {
            Some(k) => write!(f, "{k:?}#{}g{}", self.id(), self.generation()),
            None => write!(f, "Sigil({:032x})", self.0),
        }
    }
}

/// Cooked Law table index.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default)]
pub struct LawId(pub u16);

/// Cooked Affordance table index (`Lockable`, `Portable`, …).
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default)]
pub struct AffordanceId(pub u16);

/// Resource row (`mass_g`, `heat`, `stamina`, …).
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default)]
pub struct ResourceId(pub u8);

/// Local player slot. v1 is one local player; listen-server may use 0..=1.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default)]
pub struct PlayerId(pub u8);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_round_trip() {
        let s = Sigil::pack(LocusKind::Relic, 3, 42).unwrap();
        assert_eq!(s.kind(), Some(LocusKind::Relic));
        assert_eq!(s.generation(), 3);
        assert_eq!(s.id(), 42);
        assert_eq!(Sigil::from_raw(s.raw()), s);
    }

    #[test]
    fn pack_rejects_oversize_id() {
        assert!(Sigil::pack(LocusKind::Actor, 0, Sigil::ID_MASK).is_some());
        assert!(Sigil::pack(LocusKind::Actor, 0, Sigil::ID_MASK + 1).is_none());
    }

    #[test]
    fn kind_zero_is_invalid() {
        let s = Sigil::from_raw(0);
        assert_eq!(s.kind(), None);
        assert_eq!(LocusKind::from_u8(0), None);
        assert_eq!(LocusKind::Actor.as_u8(), 1);
    }

    #[test]
    fn generation_sits_between_kind_and_id() {
        let s = Sigil::pack(LocusKind::Observer, 255, 1).unwrap();
        assert_eq!(s.generation(), 255);
        assert_eq!(s.id(), 1);
        assert_eq!(s.kind(), Some(LocusKind::Observer));
    }
}
