//! Seed a [`CommitKernel`] from cooked Canon + seed facts.

use std::sync::Arc;

use klotho_author::Cooked;
use klotho_commit::CommitKernel;
use klotho_core::PlayerId;
use klotho_ir::SeedFact;
use klotho_world::World;

use crate::error::EditorError;

/// Seed a [`CommitKernel`] from packed Canon and seed facts.
pub(crate) fn kernel_from_cooked(cooked: &Cooked) -> Result<CommitKernel, EditorError> {
    let mut k = CommitKernel::new(World::new(
        Arc::new(cooked.canon.clone()),
        cooked.canon_hash,
    ));
    apply_seed(&mut k, &cooked.doc.seed)?;
    if let Some(player) = k.canon().pin("player") {
        k.bind_player(PlayerId(0), player);
    }
    Ok(k)
}

fn apply_seed(k: &mut CommitKernel, seed: &[SeedFact]) -> Result<(), EditorError> {
    for fact in seed {
        match fact {
            SeedFact::Locus { name, kind } => {
                let s = k
                    .canon()
                    .pin(name.as_str())
                    .ok_or_else(|| EditorError::Boot(format!("pin {}", name.as_str())))?;
                k.world_mut()
                    .insert_locus(s, *kind)
                    .map_err(|e| EditorError::Boot(format!("locus {}: {e}", name.as_str())))?;
            }
            SeedFact::Rel { a, rel, b } => {
                let sa = k
                    .canon()
                    .pin(a.as_str())
                    .ok_or_else(|| EditorError::Boot(format!("pin {}", a.as_str())))?;
                let sb = k
                    .canon()
                    .pin(b.as_str())
                    .ok_or_else(|| EditorError::Boot(format!("pin {}", b.as_str())))?;
                k.world_mut()
                    .add_rel(sa, *rel, sb)
                    .map_err(|e| EditorError::Boot(format!("rel: {e}")))?;
            }
            SeedFact::Qty { of, res, value } => {
                let s = k
                    .canon()
                    .pin(of.as_str())
                    .ok_or_else(|| EditorError::Boot(format!("pin {}", of.as_str())))?;
                let r = k
                    .canon()
                    .resource_id(res.as_str())
                    .ok_or_else(|| EditorError::Boot(format!("resource {}", res.as_str())))?;
                k.world_mut()
                    .set_qty(s, r, *value)
                    .map_err(|e| EditorError::Boot(format!("qty: {e}")))?;
            }
            SeedFact::Pose { of, pose } => {
                let s = k
                    .canon()
                    .pin(of.as_str())
                    .ok_or_else(|| EditorError::Boot(format!("pin {}", of.as_str())))?;
                k.world_mut()
                    .set_pose(s, *pose)
                    .map_err(|e| EditorError::Boot(format!("pose: {e}")))?;
            }
        }
    }
    Ok(())
}
