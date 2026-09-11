//! Closed v1 material tag set. One permutation; palette comes from style Intent.

/// v1 material tags. No shader graph.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
#[repr(u8)]
pub enum MaterialTag {
    /// Wood, leather, plant.
    Organic = 0,
    /// Iron, copper, tools.
    Metal = 1,
    /// Masonry, cobble, slate.
    Stone = 2,
    /// Cloth, rope.
    Cloth = 3,
    /// Fire, forge glow.
    Emissive = 4,
    /// Water, wet.
    Water = 5,
}

impl MaterialTag {
    /// Closed set, in discriminant order.
    pub const ALL: [Self; 6] = [
        Self::Organic,
        Self::Metal,
        Self::Stone,
        Self::Cloth,
        Self::Emissive,
        Self::Water,
    ];

    /// Decode a packed tag. `None` for unknown future values.
    #[must_use]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Organic),
            1 => Some(Self::Metal),
            2 => Some(Self::Stone),
            3 => Some(Self::Cloth),
            4 => Some(Self::Emissive),
            5 => Some(Self::Water),
            _ => None,
        }
    }

    /// Packed discriminant.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_set_is_six() {
        assert_eq!(MaterialTag::ALL.len(), 6);
        for (i, t) in MaterialTag::ALL.iter().enumerate() {
            assert_eq!(t.as_u8() as usize, i);
            assert_eq!(MaterialTag::from_u8(t.as_u8()), Some(*t));
        }
        assert_eq!(MaterialTag::from_u8(6), None);
    }
}
