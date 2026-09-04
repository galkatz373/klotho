//! Loci grouped by Place containment (`Rel::In`).

use std::collections::BTreeSet;
use std::fmt;

use klotho_core::LocusKind;
use klotho_ir::{IntentDoc, Name, Rel, SeedFact};

/// One seed locus in the outliner.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct LocusEntry {
    /// Authoring name.
    pub name: Name,
    /// Seed kind.
    pub kind: LocusKind,
}

/// A Place and the loci contained in it.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct PlaceGroup {
    /// The Place locus.
    pub place: LocusEntry,
    /// Members with `Rel::In` this Place, seed order.
    pub members: Vec<LocusEntry>,
}

/// Outliner rows: Place groups, then loci with no `Rel::In`.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct Outliner {
    /// Places in seed order, each with contained members.
    pub places: Vec<PlaceGroup>,
    /// Loci that are not a Place header and have no `Rel::In`.
    pub ungrouped: Vec<LocusEntry>,
}

impl fmt::Display for Outliner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for g in &self.places {
            writeln!(f, "place {}:", g.place.name)?;
            for m in &g.members {
                writeln!(f, "  {} {}", kind_label(m.kind), m.name)?;
            }
        }
        writeln!(f, "ungrouped:")?;
        for m in &self.ungrouped {
            writeln!(f, "  {} {}", kind_label(m.kind), m.name)?;
        }
        Ok(())
    }
}

fn kind_label(k: LocusKind) -> &'static str {
    match k {
        LocusKind::Actor => "actor",
        LocusKind::Place => "place",
        LocusKind::Relic => "relic",
        LocusKind::Law => "law",
        LocusKind::Beat => "beat",
        LocusKind::Chorus => "chorus",
        LocusKind::Observer => "observer",
    }
}

/// Build from seed facts. `allowed` is `None` (all) or the set of names to keep.
#[must_use]
pub fn build(doc: &IntentDoc, allowed: Option<&BTreeSet<Name>>) -> Outliner {
    let mut loci: Vec<LocusEntry> = Vec::new();
    let mut parent_of: Vec<(Name, Name)> = Vec::new();
    for fact in &doc.seed {
        match fact {
            SeedFact::Locus { name, kind } => loci.push(LocusEntry {
                name: name.clone(),
                kind: *kind,
            }),
            SeedFact::Rel { a, rel: Rel::In, b } => parent_of.push((a.clone(), b.clone())),
            _ => {}
        }
    }

    let keep = |n: &Name| allowed.is_none_or(|set| set.contains(n));

    let mut child_of: Vec<(Name, Name)> = Vec::new();
    let mut grouped: BTreeSet<Name> = BTreeSet::new();
    for (child, place) in &parent_of {
        if loci
            .iter()
            .any(|e| e.name == *place && e.kind == LocusKind::Place)
        {
            child_of.push((child.clone(), place.clone()));
            grouped.insert(child.clone());
        }
    }

    let mut places = Vec::new();
    for place in loci.iter().filter(|e| e.kind == LocusKind::Place) {
        let members: Vec<LocusEntry> = child_of
            .iter()
            .filter(|(_, p)| p == &place.name)
            .filter_map(|(c, _)| {
                loci.iter()
                    .find(|e| e.name == *c)
                    .filter(|e| keep(&e.name))
                    .cloned()
            })
            .collect();
        if keep(&place.name) || !members.is_empty() {
            places.push(PlaceGroup {
                place: place.clone(),
                members,
            });
        }
    }

    let ungrouped = loci
        .into_iter()
        .filter(|e| e.kind != LocusKind::Place && !grouped.contains(&e.name) && keep(&e.name))
        .collect();

    Outliner { places, ungrouped }
}
