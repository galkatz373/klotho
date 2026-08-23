//! Kernel spatial index (`space_ix`). Rebuildable; never a source.

use std::collections::{BTreeMap, BTreeSet};

use klotho_core::{AabbMm, IVec3};

/// Uniform grid cell size, millimetres. Power of two for cheap `div_euclid`.
pub const CELL_MM: i32 = 1024;

/// Uniform XZ grid of hulls at current pose, tagged `OpaqueClosed`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GridIndex {
    /// `(cell_x, cell_z)` → slot → opaque_closed.
    cells: BTreeMap<(i32, i32), BTreeMap<u16, bool>>,
    /// Cells occupied by each slot (for incremental unindex).
    occupied: Vec<Vec<(i32, i32)>>,
}

impl GridIndex {
    /// Empty index.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Ensure `occupied` covers `slot`.
    fn ensure_slot(&mut self, slot: u16) {
        let n = slot as usize + 1;
        if self.occupied.len() < n {
            self.occupied.resize(n, Vec::new());
        }
    }

    /// Remove a slot from every cell it occupies.
    pub fn unindex(&mut self, slot: u16) {
        if (slot as usize) >= self.occupied.len() {
            return;
        }
        let cells = core::mem::take(&mut self.occupied[slot as usize]);
        for c in cells {
            if let Some(bucket) = self.cells.get_mut(&c) {
                bucket.remove(&slot);
                if bucket.is_empty() {
                    self.cells.remove(&c);
                }
            }
        }
    }

    /// Insert `world` AABB for `slot`. Replaces any previous occupancy.
    pub fn index(&mut self, slot: u16, world: AabbMm, opaque_closed: bool) {
        self.unindex(slot);
        if world.is_empty() {
            return;
        }
        self.ensure_slot(slot);
        let cells = cells_of(world);
        for c in &cells {
            self.cells
                .entry(*c)
                .or_default()
                .insert(slot, opaque_closed);
        }
        self.occupied[slot as usize] = cells;
    }

    /// Drop everything and re-insert from `items` `(slot, world_aabb, opaque_closed)`.
    pub fn rebuild(&mut self, items: impl IntoIterator<Item = (u16, AabbMm, bool)>) {
        self.cells.clear();
        self.occupied.clear();
        for (slot, aabb, oc) in items {
            self.index(slot, aabb, oc);
        }
    }

    /// Slots whose hull may overlap `swept`. Caller maps slot → Sigil.
    /// If `opaque_closed_only`, only tagged closed-opaque hulls.
    #[must_use]
    pub fn candidates(&self, swept: AabbMm, opaque_closed_only: bool) -> BTreeSet<u16> {
        let mut out = BTreeSet::new();
        if swept.is_empty() {
            return out;
        }
        for c in cells_of(swept) {
            if let Some(bucket) = self.cells.get(&c) {
                for (&slot, &oc) in bucket {
                    if !opaque_closed_only || oc {
                        out.insert(slot);
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

    pub(crate) fn approx_bytes(&self) -> usize {
        let mut n = self.cells.len() * 16;
        for bucket in self.cells.values() {
            n += bucket.len() * 4;
        }
        for v in &self.occupied {
            n += v.len() * 8;
        }
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
