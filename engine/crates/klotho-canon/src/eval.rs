//! Predicate interpreter. Caps exceeded → [`RejectReason::Budget`].

use klotho_core::{AabbMm, AffordanceId, Mm, RejectReason, ResourceId, Sigil, SimLod};
use klotho_ir::{Channel, Cmp, Rel, SourceKind, Verb};

use crate::ast::{
    Atom, CookedSlot, PRED_OPS_PER_EVAL, PredChunk, PredOp, PredProgram, RELATED_SCAN_CAP,
    RelatedScan,
};

/// Projection the interpreter reads. `klotho-world` will implement this.
pub trait PredStore {
    /// Affordance bit on a locus.
    fn has_affordance(&self, s: Sigil, a: AffordanceId) -> bool;
    /// Relation triple.
    fn has_rel(&self, a: Sigil, r: Rel, b: Sigil) -> bool;
    /// Neighbors of `a` along `r`, in a deterministic order. Caller caps the scan.
    fn related(&self, a: Sigil, r: Rel, out: &mut Vec<Sigil>);
    /// Quantity row; missing is 0 (closed-world).
    fn qty(&self, s: Sigil, r: ResourceId) -> i32;
    /// Canonical hull at current pose. Missing → `AabbNear` is false.
    fn aabb(&self, s: Sigil) -> Option<AabbMm>;
    /// Knows table. `fact` is a Canon intern.
    fn knows(&self, mind: Sigil, fact: u16) -> bool;
    /// `RiteMachine` row for this actor + rite.
    fn rite_active(&self, actor: Sigil, rite: crate::RiteId) -> bool;
    /// Current tick is inside that WAIT channel.
    fn in_window(&self, rite: crate::RiteId, ch: Channel) -> bool;
    /// Island sleep. `None` if the locus is unknown.
    fn sleep_ticks(&self, s: Sigil) -> Option<u16>;
    /// Simulation LOD. Missing row is [`SimLod::Full`].
    fn sim_lod(&self, s: Sigil) -> SimLod;
}

/// Proposal + pin bindings for one eval. `other` is rebound by related-scans.
pub struct EvalCtx<'a, S: PredStore + ?Sized> {
    /// World / test store.
    pub store: &'a S,
    /// Acting locus (`Self`).
    pub this: Sigil,
    /// Intent target.
    pub target: Option<Sigil>,
    /// `CookedSlot::Pin(i)` → seed Sigil. `None` fails closed.
    pub pins: &'a [Option<Sigil>],
    /// Current proposal verb.
    pub verb: Verb,
    /// Who emitted the proposal.
    pub source: SourceKind,
    /// Agency channels claimed on the current proposal.
    pub claimed: &'a [Channel],
    /// Kernel-derived swept vs any OpaqueClosed hull.
    pub swept_hits_opaque_closed: bool,
}

/// Evaluate `prog`. `tick_ops` is the remaining per-tick pred-op budget.
///
/// Each eval also has a fresh [`PRED_OPS_PER_EVAL`] cap. Nested related-scan
/// evals share that per-eval cap. Exceeding either cap is
/// [`RejectReason::Budget`].
pub fn eval_pred<S: PredStore + ?Sized>(
    prog: &PredProgram,
    ctx: &EvalCtx<'_, S>,
    tick_ops: &mut u32,
) -> Result<bool, RejectReason> {
    let mut eval_ops = PRED_OPS_PER_EVAL;
    let mut scan_pc = 0usize;
    eval_chunk(
        &prog.chunk,
        prog,
        ctx,
        None,
        &mut scan_pc,
        tick_ops,
        &mut eval_ops,
    )
}

fn eval_chunk<S: PredStore + ?Sized>(
    chunk: &PredChunk,
    prog: &PredProgram,
    ctx: &EvalCtx<'_, S>,
    other: Option<Sigil>,
    scan_pc: &mut usize,
    tick_ops: &mut u32,
    eval_ops: &mut u16,
) -> Result<bool, RejectReason> {
    let mut stack: Vec<bool> = Vec::new();
    for op in &chunk.ops {
        charge(tick_ops, eval_ops)?;
        match *op {
            PredOp::PushAtom(i) => {
                let atom = match prog.atoms.get(i as usize) {
                    Some(a) => *a,
                    None => {
                        stack.push(false);
                        continue;
                    }
                };
                stack.push(eval_atom(atom, ctx, other));
            }
            PredOp::And => {
                let b = pop(&mut stack);
                let a = pop(&mut stack);
                stack.push(a && b);
            }
            PredOp::Or => {
                let b = pop(&mut stack);
                let a = pop(&mut stack);
                stack.push(a || b);
            }
            PredOp::Not => {
                let a = pop(&mut stack);
                stack.push(!a);
            }
            PredOp::ExistsRelated | PredOp::CountRelated => {
                let scan = match prog.scans.get(*scan_pc) {
                    Some(s) => s,
                    None => {
                        stack.push(false);
                        continue;
                    }
                };
                *scan_pc += 1;
                stack.push(eval_scan(scan, prog, ctx, other, tick_ops, eval_ops)?);
            }
            PredOp::Halt => return Ok(pop(&mut stack)),
        }
    }
    Ok(pop(&mut stack))
}

fn eval_scan<S: PredStore + ?Sized>(
    scan: &RelatedScan,
    prog: &PredProgram,
    ctx: &EvalCtx<'_, S>,
    outer_other: Option<Sigil>,
    tick_ops: &mut u32,
    eval_ops: &mut u16,
) -> Result<bool, RejectReason> {
    let Some(of) = resolve(scan.of, ctx, outer_other) else {
        return Ok(false);
    };
    let mut neigh = Vec::new();
    ctx.store.related(of, scan.rel, &mut neigh);
    if neigh.len() > RELATED_SCAN_CAP as usize {
        neigh.truncate(RELATED_SCAN_CAP as usize);
    }
    let mut hits: i32 = 0;
    for n in neigh {
        // Nested chunks are quantifier-free: they do not consume scan_pc.
        let mut nested_scan = 0usize;
        if eval_chunk(
            &scan.pred,
            prog,
            ctx,
            Some(n),
            &mut nested_scan,
            tick_ops,
            eval_ops,
        )? {
            hits = hits.saturating_add(1);
        }
    }
    Ok(match scan.count {
        None => hits > 0,
        Some((cmp, n)) => cmp_i32(hits, cmp, n),
    })
}

fn eval_atom<S: PredStore + ?Sized>(
    atom: Atom,
    ctx: &EvalCtx<'_, S>,
    other: Option<Sigil>,
) -> bool {
    match atom {
        Atom::Affordance(s, a) => resolve(s, ctx, other)
            .map(|s| ctx.store.has_affordance(s, a))
            .unwrap_or(false),
        Atom::Rel(a, r, b) => match (resolve(a, ctx, other), resolve(b, ctx, other)) {
            (Some(x), Some(y)) => ctx.store.has_rel(x, r, y),
            _ => false,
        },
        Atom::Qty(s, res, cmp, n) => resolve(s, ctx, other)
            .map(|s| cmp_i32(ctx.store.qty(s, res), cmp, n))
            .unwrap_or(false),
        Atom::EqVerb(v) => ctx.verb == v,
        Atom::RiteActive(id) => ctx.store.rite_active(ctx.this, id),
        Atom::AabbNear(a, b, mm) => match (resolve(a, ctx, other), resolve(b, ctx, other)) {
            (Some(x), Some(y)) => match (ctx.store.aabb(x), ctx.store.aabb(y)) {
                (Some(pa), Some(pb)) => aabb_near(pa, pb, mm),
                _ => false,
            },
            _ => false,
        },
        Atom::InWindow(id, ch) => ctx.store.in_window(id, ch),
        Atom::Knows(s, fact) => resolve(s, ctx, other)
            .map(|s| ctx.store.knows(s, fact))
            .unwrap_or(false),
        Atom::SourceIs(k) => ctx.source == k,
        Atom::AgencyClaimed(ch) => ctx.claimed.contains(&ch),
        Atom::SweptHitsOpaqueClosed => ctx.swept_hits_opaque_closed,
        Atom::IslandAwake(s) => resolve(s, ctx, other)
            .and_then(|s| ctx.store.sleep_ticks(s))
            .is_some_and(|t| t == 0),
        Atom::SelfIs(s) => resolve(s, ctx, other) == Some(ctx.this),
        Atom::TargetIs(s) => ctx.target.is_some() && resolve(s, ctx, other) == ctx.target,
        Atom::OtherIs(s) => other.is_some() && resolve(s, ctx, other) == other,
        Atom::RayHits { .. } => false,
        Atom::SimLodIs(s, lod) => {
            resolve(s, ctx, other).is_some_and(|s| ctx.store.sim_lod(s) == lod)
        }
        Atom::InPlace(s, p) => match (resolve(s, ctx, other), resolve(p, ctx, other)) {
            (Some(a), Some(b)) => ctx.store.has_rel(a, Rel::In, b),
            _ => false,
        },
    }
}

fn resolve<S: PredStore + ?Sized>(
    slot: CookedSlot,
    ctx: &EvalCtx<'_, S>,
    other: Option<Sigil>,
) -> Option<Sigil> {
    match slot {
        CookedSlot::This => Some(ctx.this),
        CookedSlot::Target => ctx.target,
        CookedSlot::Other => other,
        CookedSlot::Pin(i) => ctx.pins.get(i as usize).copied().flatten(),
    }
}

fn pop(stack: &mut Vec<bool>) -> bool {
    stack.pop().unwrap_or(false)
}

fn charge(tick_ops: &mut u32, eval_ops: &mut u16) -> Result<(), RejectReason> {
    if *tick_ops == 0 || *eval_ops == 0 {
        return Err(RejectReason::Budget);
    }
    *tick_ops -= 1;
    *eval_ops -= 1;
    Ok(())
}

fn cmp_i32(a: i32, cmp: Cmp, b: i32) -> bool {
    match cmp {
        Cmp::Lt => a < b,
        Cmp::Le => a <= b,
        Cmp::Eq => a == b,
        Cmp::Ge => a >= b,
        Cmp::Gt => a > b,
    }
}

/// Integer Euclidean distance between closed AABBs. Overlap is 0. No `f32`.
pub(crate) fn aabb_near(a: AabbMm, b: AabbMm, max: Mm) -> bool {
    if a.intersects(b) {
        return true;
    }
    let dx = axis_gap(a.min.x, a.max.x, b.min.x, b.max.x);
    let dy = axis_gap(a.min.y, a.max.y, b.min.y, b.max.y);
    let dz = axis_gap(a.min.z, a.max.z, b.min.z, b.max.z);
    let d2 = i64::from(dx) * i64::from(dx)
        + i64::from(dy) * i64::from(dy)
        + i64::from(dz) * i64::from(dz);
    let m = i64::from(max.0);
    d2 <= m.saturating_mul(m)
}

fn axis_gap(amin: i32, amax: i32, bmin: i32, bmax: i32) -> i32 {
    if amax < bmin {
        bmin.saturating_sub(amax)
    } else if bmax < amin {
        amin.saturating_sub(bmax)
    } else {
        0
    }
}

/// In-memory [`PredStore`] for tests and headless goldens before `klotho-world`.
#[derive(Clone, Debug, Default)]
pub struct MemStore {
    afford: std::collections::BTreeSet<(Sigil, AffordanceId)>,
    /// `Rel` / `Channel` are not `Ord`; keep insertion order and scan.
    rels: Vec<(Sigil, Rel, Sigil)>,
    qty: std::collections::BTreeMap<(Sigil, ResourceId), i32>,
    aabbs: std::collections::BTreeMap<Sigil, AabbMm>,
    knows: std::collections::BTreeSet<(Sigil, u16)>,
    rites: std::collections::BTreeSet<(Sigil, crate::RiteId)>,
    windows: Vec<(crate::RiteId, Channel)>,
    sleep: std::collections::BTreeMap<Sigil, u16>,
    lod: std::collections::BTreeMap<Sigil, SimLod>,
}

impl MemStore {
    /// Empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set or clear an affordance bit.
    pub fn set_affordance(&mut self, s: Sigil, a: AffordanceId, on: bool) {
        if on {
            self.afford.insert((s, a));
        } else {
            self.afford.remove(&(s, a));
        }
    }

    /// Insert a relation triple.
    pub fn add_rel(&mut self, a: Sigil, r: Rel, b: Sigil) {
        if !self.rels.contains(&(a, r, b)) {
            self.rels.push((a, r, b));
        }
    }

    /// Set a quantity row.
    pub fn set_qty(&mut self, s: Sigil, r: ResourceId, v: i32) {
        self.qty.insert((s, r), v);
    }

    /// Set a hull.
    pub fn set_aabb(&mut self, s: Sigil, box_: AabbMm) {
        self.aabbs.insert(s, box_);
    }

    /// Set a knows row.
    pub fn set_knows(&mut self, mind: Sigil, fact: u16, on: bool) {
        if on {
            self.knows.insert((mind, fact));
        } else {
            self.knows.remove(&(mind, fact));
        }
    }

    /// Set a rite-machine row.
    pub fn set_rite_active(&mut self, actor: Sigil, rite: crate::RiteId, on: bool) {
        if on {
            self.rites.insert((actor, rite));
        } else {
            self.rites.remove(&(actor, rite));
        }
    }

    /// Set a WAIT window.
    pub fn set_window(&mut self, rite: crate::RiteId, ch: Channel, on: bool) {
        let pair = (rite, ch);
        let present = self.windows.contains(&pair);
        if on && !present {
            self.windows.push(pair);
        } else if !on {
            self.windows.retain(|&w| w != pair);
        }
    }

    /// Set island sleep. `0` is awake.
    pub fn set_sleep(&mut self, s: Sigil, ticks: u16) {
        self.sleep.insert(s, ticks);
    }

    /// Set simulation LOD.
    pub fn set_sim_lod(&mut self, s: Sigil, lod: SimLod) {
        self.lod.insert(s, lod);
    }
}

impl PredStore for MemStore {
    fn has_affordance(&self, s: Sigil, a: AffordanceId) -> bool {
        self.afford.contains(&(s, a))
    }

    fn has_rel(&self, a: Sigil, r: Rel, b: Sigil) -> bool {
        self.rels.contains(&(a, r, b))
    }

    fn related(&self, a: Sigil, r: Rel, out: &mut Vec<Sigil>) {
        out.clear();
        for &(s, rel, b) in &self.rels {
            if s == a && rel == r {
                out.push(b);
            }
        }
        out.sort();
        out.dedup();
    }

    fn qty(&self, s: Sigil, r: ResourceId) -> i32 {
        self.qty.get(&(s, r)).copied().unwrap_or(0)
    }

    fn aabb(&self, s: Sigil) -> Option<AabbMm> {
        self.aabbs.get(&s).copied()
    }

    fn knows(&self, mind: Sigil, fact: u16) -> bool {
        self.knows.contains(&(mind, fact))
    }

    fn rite_active(&self, actor: Sigil, rite: crate::RiteId) -> bool {
        self.rites.contains(&(actor, rite))
    }

    fn in_window(&self, rite: crate::RiteId, ch: Channel) -> bool {
        self.windows.contains(&(rite, ch))
    }

    fn sleep_ticks(&self, s: Sigil) -> Option<u16> {
        self.sleep.get(&s).copied()
    }

    fn sim_lod(&self, s: Sigil) -> SimLod {
        self.lod.get(&s).copied().unwrap_or(SimLod::Full)
    }
}

#[cfg(test)]
mod tests {
    use klotho_core::{IVec3, LocusKind, SimLod};
    use klotho_ir::{Channel, Pred, Rel, Slot, SourceKind, Verb};

    use super::*;
    use crate::PRED_OPS_PER_TICK;
    use crate::compile::compile_pred;

    fn actor(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Actor, 0, id).unwrap()
    }

    fn relic(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Relic, 0, id).unwrap()
    }

    fn ctx<'a>(
        store: &'a MemStore,
        this: Sigil,
        target: Option<Sigil>,
        pins: &'a [Option<Sigil>],
        verb: Verb,
        claimed: &'a [Channel],
    ) -> EvalCtx<'a, MemStore> {
        EvalCtx {
            store,
            this,
            target,
            pins,
            verb,
            source: SourceKind::Player,
            claimed,
            swept_hits_opaque_closed: false,
        }
    }

    #[test]
    fn eq_verb_and_closed_world_missing_target() {
        let prog = compile_pred(&Pred::And(
            Box::new(Pred::EqVerb(Verb::Use)),
            Box::new(Pred::Affordance(
                Slot::Target,
                klotho_ir::Name::from("Lockable"),
            )),
        ))
        .unwrap();
        let store = MemStore::new();
        let this = actor(1);
        let claimed = [];
        let pins = [];
        let mut tick = PRED_OPS_PER_TICK;
        let c = ctx(&store, this, None, &pins, Verb::Use, &claimed);
        assert!(!eval_pred(&prog, &c, &mut tick).unwrap());
    }

    #[test]
    fn exists_related_key_in_hand() {
        let prog = compile_pred(&Pred::ExistsRelated {
            of: Slot::Target,
            rel: Rel::KeyedBy,
            pred: Box::new(Pred::Rel(Slot::Other, Rel::WieldedBy, Slot::This)),
        })
        .unwrap();
        let mut store = MemStore::new();
        let player = actor(1);
        let door = relic(2);
        let key = relic(3);
        store.add_rel(door, Rel::KeyedBy, key);
        store.add_rel(key, Rel::WieldedBy, player);
        let claimed = [];
        let pins = [];
        let mut tick = PRED_OPS_PER_TICK;
        let c = ctx(&store, player, Some(door), &pins, Verb::Use, &claimed);
        assert!(eval_pred(&prog, &c, &mut tick).unwrap());
    }

    #[test]
    fn related_scan_ops_hit_eval_cap() {
        let prog = compile_pred(&Pred::ExistsRelated {
            of: Slot::This,
            rel: Rel::WieldedBy,
            pred: Box::new(Pred::OtherIs(Slot::Other)),
        })
        .unwrap();
        let mut store = MemStore::new();
        let this = actor(1);
        for i in 0..64u128 {
            store.add_rel(this, Rel::WieldedBy, relic(100 + i));
        }
        let claimed = [];
        let pins = [];
        let mut tick = PRED_OPS_PER_TICK;
        let c = ctx(&store, this, None, &pins, Verb::Use, &claimed);
        assert_eq!(eval_pred(&prog, &c, &mut tick), Err(RejectReason::Budget));
    }

    #[test]
    fn tick_budget_across_evals() {
        let prog = compile_pred(&Pred::EqVerb(Verb::Use)).unwrap();
        let store = MemStore::new();
        let claimed = [];
        let pins = [];
        let c = ctx(&store, actor(1), None, &pins, Verb::Use, &claimed);
        let mut tick = 1;
        assert_eq!(eval_pred(&prog, &c, &mut tick), Err(RejectReason::Budget));
    }

    #[test]
    fn aabb_near_integer() {
        let a = AabbMm::new(
            IVec3 { x: 0, y: 0, z: 0 },
            IVec3 {
                x: 10,
                y: 10,
                z: 10,
            },
        );
        let b = AabbMm::new(
            IVec3 { x: 20, y: 0, z: 0 },
            IVec3 {
                x: 30,
                y: 10,
                z: 10,
            },
        );
        assert!(aabb_near(a, b, Mm(10)));
        assert!(!aabb_near(a, b, Mm(9)));
    }

    #[test]
    fn sim_lod_defaults_full_ray_hits_false_inplace_is_rel_in() {
        let mut store = MemStore::new();
        let this = actor(1);
        let place = relic(2);
        store.add_rel(this, Rel::In, place);
        let claimed = [];
        let pins = [];
        let mut tick = PRED_OPS_PER_TICK;
        let full = compile_pred(&Pred::SimLodIs(Slot::This, SimLod::Full)).unwrap();
        let far = compile_pred(&Pred::SimLodIs(Slot::This, SimLod::Far)).unwrap();
        {
            let c = ctx(&store, this, Some(place), &pins, Verb::Use, &claimed);
            assert!(eval_pred(&full, &c, &mut tick).unwrap());
            tick = PRED_OPS_PER_TICK;
            assert!(!eval_pred(&far, &c, &mut tick).unwrap());
        }
        store.set_sim_lod(this, SimLod::Far);
        let c = ctx(&store, this, Some(place), &pins, Verb::Use, &claimed);
        tick = PRED_OPS_PER_TICK;
        assert!(eval_pred(&far, &c, &mut tick).unwrap());
        tick = PRED_OPS_PER_TICK;
        assert!(!eval_pred(&full, &c, &mut tick).unwrap());
        tick = PRED_OPS_PER_TICK;
        let ray = compile_pred(&Pred::RayHits {
            from: Slot::This,
            dir: IVec3 { x: 0, y: 0, z: 1 },
            max: Mm(1000),
            mask: 0,
        })
        .unwrap();
        assert!(!eval_pred(&ray, &c, &mut tick).unwrap());
        tick = PRED_OPS_PER_TICK;
        let here = compile_pred(&Pred::InPlace(Slot::This, Slot::Target)).unwrap();
        assert!(eval_pred(&here, &c, &mut tick).unwrap());
        let empty = MemStore::new();
        let c_empty = ctx(&empty, this, Some(place), &pins, Verb::Use, &claimed);
        tick = PRED_OPS_PER_TICK;
        assert!(!eval_pred(&here, &c_empty, &mut tick).unwrap());
    }
}
