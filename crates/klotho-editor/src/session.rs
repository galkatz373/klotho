//! Headless Distaff session: document, cook, gizmo overlay, Pin, play.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use klotho_author::{
    Cooked, IntentDoc, Pin, apply_pin as author_apply_pin, cook_validated, load_file,
};
use klotho_commit::CommitKernel;
use klotho_core::{Budget, PoseMm, Tick};
use klotho_ir::{Name, SeedFact};
use klotho_manifest::{GpuBudget, Observer, VisualManifest};
use klotho_render::{Presenter, binds_from_cooked, extract_visual};
use klotho_ui::Pause;

use crate::dashboard::{self, CookDashboard};
use crate::error::EditorError;
use crate::inspector::{self, InspectorView};
use crate::kernel::kernel_from_cooked;
use crate::outliner::{self, Outliner};

/// Distaff GUI model. Viewport is Manifest; saving writes Pin onto the document.
pub struct EditorSession {
    doc: IntentDoc,
    cooked: Option<Cooked>,
    kernel: Option<CommitKernel>,
    /// Preview poses keyed by locus name. Recook ignores this map.
    overlay: BTreeMap<Name, PoseMm>,
    pin_reasons: BTreeMap<Name, String>,
    selection: Option<Name>,
    tag_filter: Option<Name>,
    pause: Pause,
    playing: bool,
    pin_ui: bool,
}

impl EditorSession {
    /// Hold `doc`. Call [`Self::cook`] before play or viewport present.
    #[must_use]
    pub fn new(doc: IntentDoc) -> Self {
        Self {
            doc,
            cooked: None,
            kernel: None,
            overlay: BTreeMap::new(),
            pin_reasons: BTreeMap::new(),
            selection: None,
            tag_filter: None,
            pause: Pause::new(),
            playing: false,
            pin_ui: false,
        }
    }

    /// Load RON or kdown via [`klotho_author::load_file`].
    pub fn load(path: &Path) -> Result<Self, EditorError> {
        Ok(Self::new(load_file(path)?))
    }

    /// Authoring document. Recook reads this, not live Projection.
    #[must_use]
    pub fn doc(&self) -> &IntentDoc {
        &self.doc
    }

    /// Last successful cook.
    #[must_use]
    pub fn cooked(&self) -> Option<&Cooked> {
        self.cooked.as_ref()
    }

    /// Seeded kernel, if cooked.
    #[must_use]
    pub fn kernel(&self) -> Option<&CommitKernel> {
        self.kernel.as_ref()
    }

    /// Kernel write path (play dirties, tests).
    pub fn kernel_mut(&mut self) -> Option<&mut CommitKernel> {
        self.kernel.as_mut()
    }

    /// Cook from the document and boot a kernel. Overlay is kept.
    pub fn cook(&mut self) -> Result<&Cooked, EditorError> {
        self.rebuild()?;
        self.cooked.as_ref().ok_or(EditorError::NoCook)
    }

    /// Drop unpinned overlay, then cook from the document.
    pub fn recook(&mut self) -> Result<&Cooked, EditorError> {
        self.overlay.clear();
        self.playing = false;
        self.rebuild()?;
        self.cooked.as_ref().ok_or(EditorError::NoCook)
    }

    fn rebuild(&mut self) -> Result<(), EditorError> {
        let cooked = cook_validated(&self.doc)?;
        let kernel = kernel_from_cooked(&cooked)?;
        self.cooked = Some(cooked);
        self.kernel = Some(kernel);
        Ok(())
    }

    /// Unpinned gizmo overlay.
    #[must_use]
    pub fn overlay(&self) -> &BTreeMap<Name, PoseMm> {
        &self.overlay
    }

    /// True when overlay is non-empty.
    #[must_use]
    pub fn dirty(&self) -> bool {
        !self.overlay.is_empty()
    }

    /// Seed pose for `locus`, if authored.
    #[must_use]
    pub fn seed_pose(&self, locus: &Name) -> Option<PoseMm> {
        self.doc.seed.iter().find_map(|f| match f {
            SeedFact::Pose { of, pose } if of == locus => Some(*pose),
            _ => None,
        })
    }

    /// Kernel pose for `locus` after cook/play.
    #[must_use]
    pub fn kernel_pose(&self, locus: &Name) -> Option<PoseMm> {
        let k = self.kernel.as_ref()?;
        let s = k.canon().pin(locus.as_str())?;
        k.world().view().pose(s)
    }

    /// Overlay pose if present, else seed, else kernel.
    #[must_use]
    pub fn preview_pose(&self, locus: &Name) -> Option<PoseMm> {
        self.overlay
            .get(locus)
            .copied()
            .or_else(|| self.seed_pose(locus))
            .or_else(|| self.kernel_pose(locus))
    }

    /// Move a locus in the overlay. Does not Pin and does not write Projection.
    pub fn gizmo_translate(&mut self, locus: &Name, pose: PoseMm) -> Result<(), EditorError> {
        self.require_locus(locus)?;
        self.overlay.insert(locus.clone(), pose);
        Ok(())
    }

    /// Drop the overlay without Pin.
    pub fn discard_overlay(&mut self) {
        self.overlay.clear();
    }

    /// Pin the overlay pose for `locus` into seed and rebuild from the document.
    pub fn pin_pose(&mut self, locus: &Name, reason: impl Into<String>) -> Result<(), EditorError> {
        self.require_locus(locus)?;
        let pose = self
            .overlay
            .get(locus)
            .copied()
            .ok_or_else(|| EditorError::Boot(format!("no overlay pose {}", locus.as_str())))?;
        self.apply_pin(Pin::ToSeedTrace {
            fact: SeedFact::Pose {
                of: locus.clone(),
                pose,
            },
            reason: reason.into(),
        })
    }

    /// Apply `pin` to the document. Empty reason is rejected; overlay for that locus is dropped.
    pub fn apply_pin(&mut self, pin: Pin) -> Result<(), EditorError> {
        let locus = pin_locus(&pin);
        let reason = pin_reason(&pin).to_string();
        author_apply_pin(&mut self.doc, pin)?;
        if let Some(name) = locus {
            self.pin_reasons.insert(name.clone(), reason);
            self.overlay.remove(&name);
        }
        self.rebuild()?;
        Ok(())
    }

    /// Pause while the Pin UI is open so play cannot step under an unsaved gizmo.
    pub fn open_pin_ui(&mut self) {
        self.pin_ui = true;
        self.pause.set_paused(true);
    }

    /// Close the Pin UI. Does not unpause on its own.
    pub fn close_pin_ui(&mut self) {
        self.pin_ui = false;
    }

    /// Pin UI visibility.
    #[must_use]
    pub fn pin_ui_open(&self) -> bool {
        self.pin_ui
    }

    /// Select a locus for the inspector, or clear.
    pub fn select(&mut self, locus: Option<Name>) {
        self.selection = locus;
    }

    /// Current selection.
    #[must_use]
    pub fn selection(&self) -> Option<&Name> {
        self.selection.as_ref()
    }

    /// Optional cook-binding tag filter for the outliner.
    pub fn set_tag_filter(&mut self, tag: Option<Name>) {
        self.tag_filter = tag;
    }

    /// Outliner of seed loci, grouped by Place.
    #[must_use]
    pub fn outliner(&self) -> Outliner {
        let allowed = self.allowed_names();
        outliner::build(&self.doc, allowed.as_ref())
    }

    /// Inspector for the selection.
    #[must_use]
    pub fn inspector(&self) -> Option<InspectorView> {
        inspector::inspect(&self.doc, self.selection.as_ref()?, &self.pin_reasons)
    }

    /// Cook dashboard, if cooked.
    #[must_use]
    pub fn cook_dashboard(&self) -> Option<CookDashboard> {
        self.cooked
            .as_ref()
            .map(|c| dashboard::from_cooked(c, self.dirty()))
    }

    /// Host play-in-editor. Cooks if needed. Pin UI keeps pause on.
    pub fn play(&mut self) -> Result<(), EditorError> {
        if self.kernel.is_none() {
            self.rebuild()?;
        }
        self.playing = true;
        if self.pin_ui {
            self.pause.set_paused(true);
        }
        Ok(())
    }

    /// Local pause gate.
    pub fn set_paused(&mut self, paused: bool) {
        if self.pin_ui && !paused {
            self.pause.set_paused(true);
            return;
        }
        self.pause.set_paused(paused);
    }

    /// Pause object the host honors.
    #[must_use]
    pub fn pause(&self) -> &Pause {
        &self.pause
    }

    /// Play-in-editor is hosted.
    #[must_use]
    pub fn playing(&self) -> bool {
        self.playing
    }

    /// Host should call [`Self::step`]. Pin UI or pause stops `step`.
    #[must_use]
    pub fn should_step(&self) -> bool {
        self.playing && !self.pin_ui && self.pause.should_step()
    }

    /// One kernel tick when [`Self::should_step`]. Returns whether a step ran.
    pub fn step(&mut self) -> Result<bool, EditorError> {
        if !self.should_step() {
            return Ok(false);
        }
        let kernel = self.kernel.as_mut().ok_or(EditorError::NoCook)?;
        kernel.step(Tick(1), Budget::HEARTH, &mut [])?;
        Ok(true)
    }

    /// Write a play-time pose through the kernel. Not a Pin; recook drops it.
    pub fn set_play_pose(&mut self, locus: &Name, pose: PoseMm) -> Result<(), EditorError> {
        self.require_locus(locus)?;
        let kernel = self.kernel.as_mut().ok_or(EditorError::NoCook)?;
        let s = kernel
            .canon()
            .pin(locus.as_str())
            .ok_or_else(|| EditorError::UnknownLocus(locus.clone()))?;
        kernel
            .world_mut()
            .set_pose(s, pose)
            .map_err(|e| EditorError::Boot(format!("pose {}: {e}", locus.as_str())))?;
        Ok(())
    }

    /// Extract Manifest from the last snapshot and present it.
    pub fn present(
        &mut self,
        presenter: &mut impl Presenter,
    ) -> Result<VisualManifest, EditorError> {
        if self.kernel.is_none() {
            self.rebuild()?;
        }
        let binds = {
            let cooked = self.cooked.as_ref().ok_or(EditorError::NoCook)?;
            binds_from_cooked(|n| cooked.canon.pin(n), &cooked.bindings)
        };
        let kernel = self.kernel.as_mut().ok_or(EditorError::NoCook)?;
        let snap = kernel.snapshot();
        let vis = extract_visual(&snap, &binds, false);
        presenter.present(&vis, Observer::origin(), GpuBudget::HEARTH);
        Ok(vis)
    }

    fn require_locus(&self, locus: &Name) -> Result<(), EditorError> {
        let found = self.doc.seed.iter().any(|f| {
            matches!(
                f,
                SeedFact::Locus { name, .. } if name == locus
            )
        });
        if found {
            Ok(())
        } else {
            Err(EditorError::UnknownLocus(locus.clone()))
        }
    }

    fn allowed_names(&self) -> Option<BTreeSet<Name>> {
        let tag = self.tag_filter.as_ref()?;
        let cooked = self.cooked.as_ref()?;
        let mut names = BTreeSet::new();
        for b in &cooked.bindings {
            if &b.tag == tag {
                names.insert(b.locus.clone());
            }
        }
        Some(names)
    }
}

fn pin_locus(pin: &Pin) -> Option<Name> {
    match pin {
        Pin::ToSeedTrace { fact, .. } => match fact {
            SeedFact::Locus { name, .. } => Some(name.clone()),
            SeedFact::Pose { of, .. } | SeedFact::Qty { of, .. } => Some(of.clone()),
            SeedFact::Rel { a, .. } => Some(a.clone()),
        },
        Pin::ToCanon { .. } | Pin::Reject { .. } => None,
    }
}

fn pin_reason(pin: &Pin) -> &str {
    match pin {
        Pin::ToCanon { reason, .. }
        | Pin::ToSeedTrace { reason, .. }
        | Pin::Reject { reason, .. } => reason.as_str(),
    }
}
