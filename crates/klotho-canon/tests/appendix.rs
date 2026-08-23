//! PR 04a acceptance: Appendix A RON parses; Appendix B Ash sketch parses.
//! Labeled `trade.offer` cooks; the rev-4 unlabeled list does not.

use klotho_canon::{CookError, check_rite_cfg};
use klotho_ir::{CanonDiff, RiteGraph, from_ron};

#[test]
fn appendix_a_hearth_diffs_parse() {
    let src = include_str!("../fixtures/hearth_diffs.ron");
    let d: Vec<CanonDiff> = from_ron(src).unwrap();
    assert!(d.len() >= 10);
    let trade = d
        .iter()
        .find_map(|c| match c {
            CanonDiff::AddRite(g) if g.id.as_str() == "trade.offer" => Some(g),
            _ => None,
        })
        .expect("trade.offer");
    check_rite_cfg(trade).expect("labeled trade.offer must cook");
}

#[test]
fn appendix_a_trade_offer_rev4_fails_cfg() {
    let src = include_str!("../fixtures/trade_offer_rev4.ron");
    let g: RiteGraph = from_ron(src).unwrap();
    let err = check_rite_cfg(&g).unwrap_err();
    assert!(
        matches!(
            err,
            CookError::Unreachable(_) | CookError::MissingTarget { .. }
        ),
        "{err}"
    );
}

#[test]
fn appendix_b_ash_sketch_parses() {
    let src = include_str!("../fixtures/ash.ron");
    let d: Vec<CanonDiff> = from_ron(src).unwrap();
    assert!(
        d.iter()
            .any(|c| matches!(c, CanonDiff::AddAffordance(a) if a.id.as_str() == "Hittable"))
    );
    assert!(
        d.iter()
            .any(|c| matches!(c, CanonDiff::AddLaw(l) if l.id.as_str() == "fire.hitscan"))
    );
    let fire = d
        .iter()
        .find_map(|c| match c {
            CanonDiff::AddRite(g) if g.id.as_str() == "fire" => Some(g),
            _ => None,
        })
        .unwrap();
    check_rite_cfg(fire).unwrap();
    let hit = d
        .iter()
        .find_map(|c| match c {
            CanonDiff::AddRite(g) if g.id.as_str() == "apply_hit" => Some(g),
            _ => None,
        })
        .unwrap();
    check_rite_cfg(hit).unwrap();
    let respawn = d
        .iter()
        .find_map(|c| match c {
            CanonDiff::AddRite(g) if g.id.as_str() == "respawn" => Some(g),
            _ => None,
        })
        .unwrap();
    check_rite_cfg(respawn).unwrap();
}

#[test]
fn labeled_trade_is_reachable_dag() {
    let src = include_str!("../fixtures/hearth_diffs.ron");
    let d: Vec<CanonDiff> = from_ron(src).unwrap();
    let trade = d
        .into_iter()
        .find_map(|c| match c {
            CanonDiff::AddRite(g) if g.id.as_str() == "trade.offer" => Some(g),
            _ => None,
        })
        .unwrap();
    let ops = check_rite_cfg(&trade).unwrap();
    assert!(ops.contains_key(&0));
    assert!(ops.contains_key(&10));
    assert!(!ops.contains_key(&7));
}
