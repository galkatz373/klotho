//! Read-only lookup for KAI-19 optimized-cook source maps.

use klotho_compile::{OptimizedSourceMap, SourceMapEntry};

/// Debug view over a cook-owned source map. This is never packaged into the
/// default game artifact.
pub struct OptimizationMap<'a> {
    map: &'a OptimizedSourceMap,
}

impl<'a> OptimizationMap<'a> {
    /// Borrow a source map emitted by the whole-title cook.
    #[must_use]
    pub const fn new(map: &'a OptimizedSourceMap) -> Self {
        Self { map }
    }

    /// Resolve a stable semantic row back to its immutable authoring anchor.
    #[must_use]
    pub fn resolve(&self, table: &str, stable_id: u16) -> Option<&'a SourceMapEntry> {
        self.map
            .entries
            .iter()
            .find(|entry| entry.table == table && entry.stable_id == stable_id)
    }
}

#[cfg(test)]
mod tests {
    use klotho_compile::{OptimizedSourceMap, SourceMapEntry};
    use klotho_ir::AnchorId;

    use super::*;

    #[test]
    fn stable_row_resolves_without_runtime_state() {
        let anchor = AnchorId::derive(b"debug", b"law:a");
        let map = OptimizedSourceMap {
            entries: vec![SourceMapEntry {
                table: "law",
                stable_id: 4,
                anchor,
                span: None,
            }],
        };
        assert_eq!(
            OptimizationMap::new(&map).resolve("law", 4).unwrap().anchor,
            anchor
        );
        assert!(OptimizationMap::new(&map).resolve("rite", 4).is_none());
    }
}
