//! Bounded material-graph compile and permutation pruning (KAI-17).
//!
//! The graph is cook input. The output is a shader permutation bitset consumed
//! by the presenter. It is never a gameplay graph and never a Projection column.

use std::collections::BTreeSet;

use klotho_ir::{MaterialGraph, MaterialNode, QualityTier};

use crate::error::CompileError;

/// Compiled shader permutation. Bits are stable across OS.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct ShaderPerm {
    /// Sampled albedo.
    pub albedo_tex: bool,
    /// Metalness/roughness from the graph (not a constant).
    pub metal_rough: bool,
    /// Emissive output is bound.
    pub emissive: bool,
    /// Alpha-tested / masked pass.
    pub alpha_test: bool,
}

impl ShaderPerm {
    /// Packed 4-bit id.
    #[must_use]
    pub const fn bits(self) -> u8 {
        (self.albedo_tex as u8)
            | ((self.metal_rough as u8) << 1)
            | ((self.emissive as u8) << 2)
            | ((self.alpha_test as u8) << 3)
    }

    /// Unlit constant albedo.
    pub const UNLIT: Self = Self {
        albedo_tex: false,
        metal_rough: false,
        emissive: false,
        alpha_test: false,
    };
}

/// Compile a validated graph to a permutation. Forward refs already failed.
pub fn compile_material(graph: &MaterialGraph) -> Result<ShaderPerm, CompileError> {
    graph
        .validate()
        .map_err(|e| CompileError::Header(e.to_string()))?;
    let mut albedo_tex = false;
    let mut metal_rough = false;
    if uses_sample(graph, graph.albedo) {
        albedo_tex = true;
    }
    if uses_sample(graph, graph.metalness) || uses_sample(graph, graph.roughness) {
        metal_rough = true;
    }
    if !matches!(
        graph.nodes[graph.metalness as usize],
        MaterialNode::Constant { .. }
    ) || !matches!(
        graph.nodes[graph.roughness as usize],
        MaterialNode::Constant { .. }
    ) {
        metal_rough = true;
    }
    Ok(ShaderPerm {
        albedo_tex,
        metal_rough,
        emissive: graph.emissive.is_some(),
        alpha_test: graph.alpha.is_some(),
    })
}

fn uses_sample(graph: &MaterialGraph, idx: u8) -> bool {
    match graph.nodes.get(idx as usize) {
        Some(MaterialNode::Sample { .. }) => true,
        Some(MaterialNode::Mul { a, b }) => uses_sample(graph, *a) || uses_sample(graph, *b),
        Some(MaterialNode::Lerp { a, b, t }) => {
            uses_sample(graph, *a) || uses_sample(graph, *b) || uses_sample(graph, *t)
        }
        Some(MaterialNode::Clamp { x, .. }) => uses_sample(graph, *x),
        _ => false,
    }
}

/// Keep only permutations that authored graphs actually use, then apply the tier.
#[must_use]
pub fn prune_permutations(used: &[ShaderPerm], tier: QualityTier) -> BTreeSet<ShaderPerm> {
    let mut out = BTreeSet::new();
    for perm in used {
        let mut p = *perm;
        match tier {
            QualityTier::Low => {
                p.metal_rough = false;
                p.emissive = false;
            }
            QualityTier::Medium => {
                p.emissive = false;
            }
            QualityTier::High => {}
        }
        out.insert(p);
    }
    if out.is_empty() {
        out.insert(ShaderPerm::UNLIT);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use klotho_ir::{MaterialGraph, MaterialNode, Name};

    #[test]
    fn organic_compiles_unlit_constant() {
        let p = compile_material(&MaterialGraph::organic()).unwrap();
        assert_eq!(p, ShaderPerm::UNLIT);
        assert_eq!(p.bits(), 0);
    }

    #[test]
    fn sample_and_emissive_set_bits() {
        let g = MaterialGraph {
            id: Name::from("hero"),
            nodes: vec![
                MaterialNode::Sample { slot: 0 },
                MaterialNode::Constant {
                    rgb_milli: [200, 200, 200],
                },
                MaterialNode::Constant {
                    rgb_milli: [400, 400, 400],
                },
                MaterialNode::Constant {
                    rgb_milli: [800, 400, 100],
                },
            ],
            albedo: 0,
            metalness: 1,
            roughness: 2,
            emissive: Some(3),
            alpha: None,
        };
        let p = compile_material(&g).unwrap();
        assert!(p.albedo_tex);
        assert!(p.emissive);
        assert!(!p.alpha_test);
    }

    #[test]
    fn prune_drops_unused_and_strips_by_tier() {
        let hero = ShaderPerm {
            albedo_tex: true,
            metal_rough: true,
            emissive: true,
            alpha_test: false,
        };
        let high = prune_permutations(&[hero], QualityTier::High);
        assert!(high.contains(&hero));
        let medium = prune_permutations(&[hero], QualityTier::Medium);
        assert!(medium.iter().all(|p| !p.emissive));
        let low = prune_permutations(&[hero], QualityTier::Low);
        assert!(low.iter().all(|p| !p.emissive && !p.metal_rough));
        let empty = prune_permutations(&[], QualityTier::High);
        assert_eq!(empty.len(), 1);
        assert!(empty.contains(&ShaderPerm::UNLIT));
    }

    #[test]
    fn unused_combo_is_not_emitted() {
        let only = ShaderPerm {
            albedo_tex: true,
            metal_rough: false,
            emissive: false,
            alpha_test: false,
        };
        let kept = prune_permutations(&[only], QualityTier::High);
        assert_eq!(kept.len(), 1);
        assert!(!kept.contains(&ShaderPerm {
            albedo_tex: true,
            metal_rough: true,
            emissive: true,
            alpha_test: true,
        }));
    }
}
