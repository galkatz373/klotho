//! Cook dashboard summary: hash, bindings, grains, dirty overlay, licenses.

use std::fmt;

use klotho_author::Cooked;
use klotho_core::Hash;

/// Cook summary: hash, bindings, grains, overlay dirty, license coverage.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct CookDashboard {
    /// Cook digest of the last successful cook.
    pub cook_hash: Hash,
    /// Seed locus → kitbash bindings.
    pub bindings: usize,
    /// Cooked grain blobs.
    pub grains: usize,
    /// True when an unpinned gizmo overlay is present.
    pub dirty: bool,
    /// Provenance DAG node count.
    pub license_nodes: usize,
    /// Nodes whose license is not exportable (`LicenseSpan::Unknown`).
    pub license_unknown: usize,
}

impl CookDashboard {
    /// True when every provenance node is exportable.
    #[must_use]
    pub const fn exportable(&self) -> bool {
        self.license_unknown == 0
    }
}

impl fmt::Display for CookDashboard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "cook_hash={}", self.cook_hash)?;
        writeln!(f, "bindings={}", self.bindings)?;
        writeln!(f, "grains={}", self.grains)?;
        writeln!(f, "dirty={}", self.dirty)?;
        writeln!(f, "license_nodes={}", self.license_nodes)?;
        writeln!(f, "license_unknown={}", self.license_unknown)?;
        Ok(())
    }
}

/// Summarize `cooked` plus whether the gizmo overlay is dirty.
#[must_use]
pub fn from_cooked(cooked: &Cooked, dirty: bool) -> CookDashboard {
    let license_nodes = cooked.dag.len();
    let license_unknown = cooked
        .dag
        .iter()
        .filter(|n| !n.license.is_exportable())
        .count();
    CookDashboard {
        cook_hash: cooked.cook_hash,
        bindings: cooked.bindings.len(),
        grains: cooked.grains.len(),
        dirty,
        license_nodes,
        license_unknown,
    }
}
