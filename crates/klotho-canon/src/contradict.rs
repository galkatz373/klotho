//! Cook-time contradiction on the **tiny fragment**: quantifier-free And/Or/Not
//! over opaque atoms (including `ExistsRelated` / `CountRelated` as leaves).
//! Plus the v1 `Lockable` key-or-rite admission check.

use klotho_ir::{Law, LawBody, Name, Pred, Rel};

use crate::compile::desugar;
use crate::error::CookError;

const LOCKABLE: &str = "Lockable";
const SAT_ATOMS: usize = 12;

/// Reject unsatisfiable `must`, pairwise Law contradictions, and Lockable
/// without a key-or-rite admission pred.
pub(crate) fn check_fragment(laws: &[(Name, Law)], has_lockable: bool) -> Result<(), CookError> {
    if has_lockable && !has_key_or_rite(laws) {
        return Err(CookError::LockableNeedsKeyOrRite);
    }
    let mut pred_laws: Vec<(Name, Pred, Pred)> = Vec::new();
    for (name, law) in laws {
        let when = desugar(&law.when);
        if let LawBody::Pred { must, .. } = &law.body {
            let must = desugar(must);
            if unsat(&must) {
                return Err(CookError::Contradiction(name.0.clone()));
            }
            pred_laws.push((name.clone(), when, must));
        }
    }
    for i in 0..pred_laws.len() {
        for j in (i + 1)..pred_laws.len() {
            let (na, wa, ma) = &pred_laws[i];
            let (nb, wb, mb) = &pred_laws[j];
            if !equivalent(wa, wb) {
                continue;
            }
            let both = Pred::And(Box::new(ma.clone()), Box::new(mb.clone()));
            if unsat(&both) {
                return Err(CookError::Contradiction(format!("{}|{}", na.0, nb.0)));
            }
        }
    }
    Ok(())
}

fn has_key_or_rite(laws: &[(Name, Law)]) -> bool {
    laws.iter().any(|(_, law)| {
        mentions_lockable(&law.when)
            && matches!(&law.body, LawBody::Pred { must, .. } if is_key_or_rite(must))
    })
}

fn mentions_lockable(p: &Pred) -> bool {
    match p {
        Pred::Affordance(_, n) => n.as_str() == LOCKABLE,
        Pred::And(a, b) | Pred::Or(a, b) => mentions_lockable(a) || mentions_lockable(b),
        Pred::Not(a) => mentions_lockable(a),
        Pred::ExistsRelated { pred, .. } | Pred::CountRelated { pred, .. } => {
            mentions_lockable(pred)
        }
        _ => false,
    }
}

/// `must` is an Or-tree containing both a KeyedBy scan and a `RiteActive`.
fn is_key_or_rite(must: &Pred) -> bool {
    let mut key = false;
    let mut rite = false;
    walk_or(must, &mut key, &mut rite);
    key && rite
}

fn walk_or(p: &Pred, key: &mut bool, rite: &mut bool) {
    match p {
        Pred::Or(a, b) => {
            walk_or(a, key, rite);
            walk_or(b, key, rite);
        }
        Pred::ExistsRelated {
            rel: Rel::KeyedBy,
            pred,
            ..
        } if is_wielded(pred) => *key = true,
        Pred::RiteActive(_) => *rite = true,
        _ => {}
    }
}

fn is_wielded(p: &Pred) -> bool {
    matches!(p, Pred::Rel(_, Rel::WieldedBy, _))
}

enum Prop {
    Atom(u32),
    And(Box<Prop>, Box<Prop>),
    Or(Box<Prop>, Box<Prop>),
    Not(Box<Prop>),
}

fn to_prop(p: &Pred, leaves: &mut Vec<Pred>) -> Prop {
    match p {
        Pred::And(a, b) => Prop::And(Box::new(to_prop(a, leaves)), Box::new(to_prop(b, leaves))),
        Pred::Or(a, b) => Prop::Or(Box::new(to_prop(a, leaves)), Box::new(to_prop(b, leaves))),
        Pred::Not(a) => Prop::Not(Box::new(to_prop(a, leaves))),
        other => {
            let id = if let Some(i) = leaves.iter().position(|q| q == other) {
                i as u32
            } else {
                let i = leaves.len() as u32;
                leaves.push(other.clone());
                i
            };
            Prop::Atom(id)
        }
    }
}

fn eval_prop(p: &Prop, bits: u64) -> bool {
    match p {
        Prop::Atom(i) => ((bits >> i) & 1) == 1,
        Prop::And(a, b) => eval_prop(a, bits) && eval_prop(b, bits),
        Prop::Or(a, b) => eval_prop(a, bits) || eval_prop(b, bits),
        Prop::Not(a) => !eval_prop(a, bits),
    }
}

fn unsat(p: &Pred) -> bool {
    let mut leaves = Vec::new();
    let prop = to_prop(p, &mut leaves);
    let n = leaves.len();
    if n > SAT_ATOMS {
        return syntactic_and_conflict(&prop);
    }
    let max = 1u64 << n;
    for bits in 0..max {
        if eval_prop(&prop, bits) {
            return false;
        }
    }
    true
}

fn equivalent(a: &Pred, b: &Pred) -> bool {
    let xor = Pred::Or(
        Box::new(Pred::And(
            Box::new(a.clone()),
            Box::new(Pred::Not(Box::new(b.clone()))),
        )),
        Box::new(Pred::And(
            Box::new(b.clone()),
            Box::new(Pred::Not(Box::new(a.clone()))),
        )),
    );
    unsat(&xor)
}

fn syntactic_and_conflict(p: &Prop) -> bool {
    let mut pos = Vec::new();
    let mut neg = Vec::new();
    flatten_and(p, false, &mut pos, &mut neg);
    for a in &pos {
        if neg.contains(a) {
            return true;
        }
    }
    false
}

fn flatten_and(p: &Prop, negated: bool, pos: &mut Vec<u32>, neg: &mut Vec<u32>) {
    match p {
        Prop::And(a, b) if !negated => {
            flatten_and(a, false, pos, neg);
            flatten_and(b, false, pos, neg);
        }
        Prop::Not(inner) => flatten_and(inner, !negated, pos, neg),
        Prop::Atom(i) => {
            if negated {
                neg.push(*i);
            } else {
                pos.push(*i);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use klotho_ir::{Cmp, Law, LawBody, Pred, Slot, Verb};

    use super::*;

    fn n(s: &str) -> Name {
        Name::from(s)
    }

    fn law(id: &str, when: Pred, must: Pred) -> (Name, Law) {
        (
            n(id),
            Law {
                id: n(id),
                when,
                body: LawBody::Pred { must, ought: None },
            },
        )
    }

    #[test]
    fn unsat_must_is_contradiction() {
        let p = Pred::And(
            Box::new(Pred::EqVerb(Verb::Use)),
            Box::new(Pred::Not(Box::new(Pred::EqVerb(Verb::Use)))),
        );
        let laws = [law("bad", Pred::EqVerb(Verb::Use), p)];
        assert!(matches!(
            check_fragment(&laws, false),
            Err(CookError::Contradiction(_))
        ));
    }

    #[test]
    fn pairwise_must_conflict() {
        let laws = [
            law("a", Pred::EqVerb(Verb::Use), Pred::EqVerb(Verb::Use)),
            law(
                "b",
                Pred::EqVerb(Verb::Use),
                Pred::Not(Box::new(Pred::EqVerb(Verb::Use))),
            ),
        ];
        assert!(matches!(
            check_fragment(&laws, false),
            Err(CookError::Contradiction(_))
        ));
    }

    #[test]
    fn lockable_without_admission_fails() {
        let laws = [law(
            "x",
            Pred::EqVerb(Verb::Use),
            Pred::Qty(Slot::This, n("stamina"), Cmp::Ge, 1),
        )];
        assert_eq!(
            check_fragment(&laws, true),
            Err(CookError::LockableNeedsKeyOrRite)
        );
    }
}
