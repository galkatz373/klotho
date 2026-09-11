//! Kernel spatial index (`space_ix`). Rebuildable; never a source.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use klotho_core::{AabbMm, IVec3, MAX_LOCI_PROCESS, PackedIx, Sigil};

use crate::cow::CowCol;

fn packed_in_range(ix: PackedIx) -> bool {
    (ix as usize) < MAX_LOCI_PROCESS
}

/// Uniform grid cell size, millimetres. Power of two for cheap `div_euclid`.
pub const CELL_MM: i32 = 1024;

/// Uniform XZ grid of hulls at current pose, tagged `OpaqueClosed`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GridIndex {
    /// `(cell_x, cell_z)` → packed index → opaque_closed.
    cells: BTreeMap<(i32, i32), BTreeMap<PackedIx, bool>>,
    /// Cells occupied by each packed index (for incremental unindex).
    occupied: Vec<Vec<(i32, i32)>>,
}

impl GridIndex {
    /// Empty index.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn ensure_ix(&mut self, ix: PackedIx) {
        if !packed_in_range(ix) {
            return;
        }
        let n = ix as usize + 1;
        if self.occupied.len() < n {
            self.occupied.reserve(n - self.occupied.len());
            self.occupied.resize(n, Vec::new());
        }
    }

    /// Remove a packed index from every cell it occupies.
    pub fn unindex(&mut self, ix: PackedIx) {
        if (ix as usize) >= self.occupied.len() {
            return;
        }
        let cells = core::mem::take(&mut self.occupied[ix as usize]);
        for c in cells {
            if let Some(bucket) = self.cells.get_mut(&c) {
                bucket.remove(&ix);
                if bucket.is_empty() {
                    self.cells.remove(&c);
                }
            }
        }
    }

    /// Insert `world` AABB for `ix`. Replaces any previous occupancy.
    ///
    /// `ix` is a dense packed row (`0..len`). [`MAX_LOCI_PROCESS`] and above
    /// are ignored so a lone `PackedIx::MAX` cannot allocate the occupancy table.
    pub fn index(&mut self, ix: PackedIx, world: AabbMm, opaque_closed: bool) {
        if !packed_in_range(ix) {
            return;
        }
        self.unindex(ix);
        if world.is_empty() {
            return;
        }
        self.ensure_ix(ix);
        let cells = cells_of(world);
        for c in &cells {
            self.cells.entry(*c).or_default().insert(ix, opaque_closed);
        }
        self.occupied[ix as usize] = cells;
    }

    /// Drop everything and re-insert from `items` `(ix, world_aabb, opaque_closed)`.
    pub fn rebuild(&mut self, items: impl IntoIterator<Item = (PackedIx, AabbMm, bool)>) {
        self.cells.clear();
        self.occupied.clear();
        for (ix, aabb, oc) in items {
            self.index(ix, aabb, oc);
        }
    }

    /// Packed indices whose hull may overlap `swept`. Caller maps index → Sigil.
    /// If `opaque_closed_only`, only tagged closed-opaque hulls.
    #[must_use]
    pub fn candidates(&self, swept: AabbMm, opaque_closed_only: bool) -> BTreeSet<PackedIx> {
        let mut out = BTreeSet::new();
        if swept.is_empty() {
            return out;
        }
        for c in cells_of(swept) {
            if let Some(bucket) = self.cells.get(&c) {
                for (&ix, &oc) in bucket {
                    if !opaque_closed_only || oc {
                        out.insert(ix);
                    }
                }
            }
        }
        out
    }

    /// Occupied cell count (debug / tests).
    #[must_use]
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    #[cfg(test)]
    fn occupancy_rows(&self) -> usize {
        self.occupied.len()
    }

    pub(crate) fn approx_bytes(&self) -> usize {
        let mut n = self.cells.len() * 16;
        for bucket in self.cells.values() {
            n += bucket.len() * 8;
        }
        for v in &self.occupied {
            n += v.len() * 8;
        }
        n
    }
}

/// Per-Place grids plus a coarse Place AABB map. Unplaced loci use `unplaced`.
#[derive(Clone, Debug, Default)]
pub struct PlaceIndex {
    unplaced: Arc<GridIndex>,
    places: BTreeMap<Sigil, Arc<GridIndex>>,
    bounds: BTreeMap<Sigil, AabbMm>,
    /// Current posed hulls in each Place, used to shrink [`Self::bounds`].
    place_hulls: BTreeMap<Sigil, BTreeMap<PackedIx, AabbMm>>,
    /// Place grid that currently holds each packed index (`None` = unplaced).
    home: CowCol<Option<Sigil>>,
}

impl PlaceIndex {
    /// Empty index.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Grid for loci with no Place.
    #[must_use]
    pub fn unplaced(&self) -> &GridIndex {
        &self.unplaced
    }

    /// Grid for `place`, if any hulls have been indexed there.
    #[must_use]
    pub fn grid(&self, place: Sigil) -> Option<&GridIndex> {
        self.places.get(&place).map(Arc::as_ref)
    }

    /// Coarse AABB covering hulls last indexed into `place`.
    #[must_use]
    pub fn bounds(&self, place: Sigil) -> Option<AabbMm> {
        self.bounds.get(&place).copied()
    }

    /// Number of Place grids that currently hold at least one hull.
    #[must_use]
    pub fn place_count(&self) -> usize {
        self.places.len()
    }

    pub(crate) fn ensure(&mut self, ix: PackedIx) {
        if !packed_in_range(ix) {
            return;
        }
        while self.home.len() <= ix as usize {
            self.home.push(None);
        }
    }

    fn set_place_bounds(&mut self, place: Sigil) {
        let u = self.place_hulls.get(&place).and_then(|hulls| {
            let mut u: Option<AabbMm> = None;
            for &aabb in hulls.values() {
                u = Some(match u {
                    None => aabb,
                    Some(prev) => prev.union(aabb),
                });
            }
            u
        });
        match u {
            Some(b) => {
                self.bounds.insert(place, b);
            }
            None => {
                self.place_hulls.remove(&place);
                self.places.remove(&place);
                self.bounds.remove(&place);
            }
        }
    }

    fn forget_place_hull(&mut self, place: Sigil, ix: PackedIx) {
        if let Some(hulls) = self.place_hulls.get_mut(&place) {
            hulls.remove(&ix);
        }
        self.set_place_bounds(place);
    }

    /// Remove `ix` from whichever grid currently holds it.
    pub fn unindex(&mut self, ix: PackedIx) {
        if (ix as usize) >= self.home.len() {
            return;
        }
        let prev = self.home.get(ix as usize).copied().flatten();
        self.home.set(ix as usize, None);
        if let Some(p) = prev {
            if let Some(g) = self.places.get_mut(&p) {
                Arc::make_mut(g).unindex(ix);
            }
            self.forget_place_hull(p, ix);
        } else {
            Arc::make_mut(&mut self.unplaced).unindex(ix);
        }
    }

    /// Insert `world` AABB for `ix` into `place` (or the unplaced grid).
    ///
    /// `ix` is a dense packed row. [`MAX_LOCI_PROCESS`] and above are ignored.
    pub fn index(
        &mut self,
        ix: PackedIx,
        world: AabbMm,
        opaque_closed: bool,
        place: Option<Sigil>,
    ) {
        if !packed_in_range(ix) {
            return;
        }
        self.unindex(ix);
        if world.is_empty() {
            return;
        }
        self.ensure(ix);
        if let Some(p) = place {
            let g = self
                .places
                .entry(p)
                .or_insert_with(|| Arc::new(GridIndex::new()));
            Arc::make_mut(g).index(ix, world, opaque_closed);
            self.place_hulls.entry(p).or_default().insert(ix, world);
            // Grow only; unindex rescans remaining hulls to shrink.
            match self.bounds.get(&p).copied() {
                Some(b) => {
                    self.bounds.insert(p, b.union(world));
                }
                None => {
                    self.bounds.insert(p, world);
                }
            }
            self.home.set(ix as usize, Some(p));
        } else {
            Arc::make_mut(&mut self.unplaced).index(ix, world, opaque_closed);
            self.home.set(ix as usize, None);
        }
    }

    /// Drop everything and re-insert from `items`.
    pub fn rebuild(
        &mut self,
        items: impl IntoIterator<Item = (PackedIx, AabbMm, bool, Option<Sigil>)>,
    ) {
        *self = Self::default();
        for (ix, aabb, oc, place) in items {
            self.index(ix, aabb, oc, place);
        }
    }

    /// Packed indices whose hull may overlap `swept`.
    ///
    /// Unions the unplaced grid with every Place whose coarse AABB intersects
    /// `swept`. Admission uses this union. Per-Place isolation is
    /// [`Self::grid`] / [`Self::unplaced`].
    #[must_use]
    pub fn candidates(&self, swept: AabbMm, opaque_closed_only: bool) -> BTreeSet<PackedIx> {
        let mut out = self.unplaced.candidates(swept, opaque_closed_only);
        for (place, bounds) in &self.bounds {
            if bounds.intersects(swept) {
                if let Some(g) = self.places.get(place) {
                    out.extend(g.candidates(swept, opaque_closed_only));
                }
            }
        }
        out
    }

    pub(crate) fn approx_bytes(&self) -> usize {
        let mut n = self.unplaced.approx_bytes();
        n += self.home.approx_bytes();
        n += self.places.len() * 16;
        for g in self.places.values() {
            n += g.approx_bytes();
        }
        n += self.bounds.len() * 32;
        n += self.place_hulls.len() * 16;
        n
    }
}

/// Translate a local hull by a millimetre translation (yaw ignored: hulls that
/// swing must ship a new local AABB).
#[must_use]
pub const fn world_aabb(local: AabbMm, translation: IVec3) -> AabbMm {
    AabbMm {
        min: local.min.wrapping_add(translation),
        max: local.max.wrapping_add(translation),
    }
}

fn cells_of(aabb: AabbMm) -> Vec<(i32, i32)> {
    let x0 = aabb.min.x.div_euclid(CELL_MM);
    let x1 = aabb.max.x.div_euclid(CELL_MM);
    let z0 = aabb.min.z.div_euclid(CELL_MM);
    let z1 = aabb.max.z.div_euclid(CELL_MM);
    let mut out = Vec::new();
    let mut x = x0;
    while x <= x1 {
        let mut z = z0;
        while z <= z1 {
            out.push((x, z));
            z += 1;
        }
        x += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use klotho_core::LocusKind;

    fn box_at(x: i32) -> AabbMm {
        AabbMm::new(
            IVec3 {
                x: x - 100,
                y: 0,
                z: -100,
            },
            IVec3 {
                x: x + 100,
                y: 500,
                z: 100,
            },
        )
    }

    #[test]
    fn grid_index_refuses_ix_at_process_cap() {
        let mut g = GridIndex::new();
        g.index(0, box_at(0), false);
        let before = g.occupancy_rows();
        g.index(MAX_LOCI_PROCESS as PackedIx, box_at(0), false);
        g.index(PackedIx::MAX, box_at(0), false);
        assert_eq!(g.occupancy_rows(), before);
        let hits = g.candidates(box_at(0), false);
        assert!(hits.contains(&0));
        assert!(!hits.contains(&(MAX_LOCI_PROCESS as PackedIx)));
        assert!(!hits.contains(&PackedIx::MAX));
    }

    #[test]
    fn place_index_refuses_ix_at_process_cap() {
        let mut ix = PlaceIndex::new();
        let p = Sigil::pack(LocusKind::Place, 0, 1).unwrap();
        ix.index(MAX_LOCI_PROCESS as PackedIx, box_at(0), false, Some(p));
        ix.index(PackedIx::MAX, box_at(0), false, None);
        assert_eq!(ix.place_count(), 0);
        assert!(ix.grid(p).is_none());
        assert!(ix.unplaced().candidates(box_at(0), false).is_empty());
    }

    #[test]
    fn place_index_isolates_colocated_places() {
        let mut ix = PlaceIndex::new();
        let a = Sigil::pack(LocusKind::Place, 0, 1).unwrap();
        let b = Sigil::pack(LocusKind::Place, 0, 2).unwrap();
        ix.index(0, box_at(0), false, Some(a));
        ix.index(1, box_at(0), false, Some(b));
        ix.index(2, box_at(0), false, None);
        let swept = box_at(0);
        let unplaced = ix.unplaced().candidates(swept, false);
        assert!(unplaced.contains(&2));
        assert!(!unplaced.contains(&0));
        assert!(!unplaced.contains(&1));
        let ga = ix.grid(a).unwrap().candidates(swept, false);
        assert!(ga.contains(&0));
        assert!(!ga.contains(&1) && !ga.contains(&2));
        let gb = ix.grid(b).unwrap().candidates(swept, false);
        assert!(gb.contains(&1));
        assert!(!gb.contains(&0) && !gb.contains(&2));
        let all = ix.candidates(swept, false);
        assert!(all.contains(&0) && all.contains(&1) && all.contains(&2));
    }

    #[test]
    fn place_index_drops_empty_and_shrinks_bounds() {
        let mut ix = PlaceIndex::new();
        let a = Sigil::pack(LocusKind::Place, 0, 1).unwrap();
        ix.index(0, box_at(0), false, Some(a));
        ix.index(1, box_at(1_000_000), false, Some(a));
        assert_eq!(ix.place_count(), 1);
        let wide = ix.bounds(a).unwrap();
        ix.unindex(1);
        let tight = ix.bounds(a).unwrap();
        assert!(tight.max.x < wide.max.x);
        assert_eq!(ix.place_count(), 1);
        ix.unindex(0);
        assert_eq!(ix.place_count(), 0);
        assert!(ix.grid(a).is_none());
        assert!(ix.bounds(a).is_none());
    }

    #[test]
    fn place_index_keeps_distant_places_apart() {
        let mut ix = PlaceIndex::new();
        let a = Sigil::pack(LocusKind::Place, 0, 1).unwrap();
        let b = Sigil::pack(LocusKind::Place, 0, 2).unwrap();
        ix.index(0, box_at(0), false, Some(a));
        ix.index(1, box_at(1_000_000), false, Some(b));
        assert_eq!(ix.place_count(), 2);
        let near = ix.candidates(box_at(0), false);
        assert!(near.contains(&0));
        assert!(!near.contains(&1));
        let far = ix.candidates(box_at(1_000_000), false);
        assert!(far.contains(&1));
        assert!(!far.contains(&0));
        assert!(ix.bounds(a).is_some());
    }
}
