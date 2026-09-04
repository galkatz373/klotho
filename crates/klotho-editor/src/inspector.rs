//! Selection inspector: seed kind, rels, qtys, and last Pin reason.

use std::collections::BTreeMap;
use std::fmt;

use klotho_core::LocusKind;
use klotho_ir::{IntentDoc, Name, Rel, SeedFact};

/// Author-facing facts for one selected locus.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct InspectorView {
    /// Selected name.
    pub name: Name,
    /// Seed kind.
    pub kind: LocusKind,
    /// Seed relations that mention this locus.
    pub rels: Vec<(Name, Rel, Name)>,
    /// Seed quantities on this locus `(resource, value)`.
    pub qtys: Vec<(Name, i32)>,
    /// Last Pin reason recorded by the session for this locus.
    pub last_pin_reason: Option<String>,
}

impl fmt::Display for InspectorView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "locus {} kind={:?}", self.name, self.kind)?;
        for (a, rel, b) in &self.rels {
            writeln!(f, "rel {} {} {}", a, rel.as_str(), b)?;
        }
        for (res, v) in &self.qtys {
            writeln!(f, "qty {res} {v}")?;
        }
        if let Some(r) = &self.last_pin_reason {
            writeln!(f, "pin {r}")?;
        }
        Ok(())
    }
}

/// Seed kind / rels / qtys and last Pin reason. `None` if that locus is not seeded.
#[must_use]
pub fn inspect(
    doc: &IntentDoc,
    name: &Name,
    pin_reasons: &BTreeMap<Name, String>,
) -> Option<InspectorView> {
    let kind = doc.seed.iter().find_map(|f| match f {
        SeedFact::Locus { name: n, kind } if n == name => Some(*kind),
        _ => None,
    })?;
    let mut rels = Vec::new();
    let mut qtys = Vec::new();
    for fact in &doc.seed {
        match fact {
            SeedFact::Rel { a, rel, b } if a == name || b == name => {
                rels.push((a.clone(), *rel, b.clone()));
            }
            SeedFact::Qty { of, res, value } if of == name => {
                qtys.push((res.clone(), *value));
            }
            _ => {}
        }
    }
    Some(InspectorView {
        name: name.clone(),
        kind,
        rels,
        qtys,
        last_pin_reason: pin_reasons.get(name).cloned(),
    })
}
