//! Typed view of legal rejects on a delta / debug event.

use klotho_core::RejectReason;
use klotho_trace::{ProposalKind, TraceDelta};

use crate::event::DebugEvent;

/// Rejects recorded on a kernel delta.
#[must_use]
pub fn inspect_rejects(delta: &TraceDelta) -> &[(ProposalKind, RejectReason)] {
    &delta.rejects
}

/// Rejects recorded on a replay event.
#[must_use]
pub fn inspect_event(ev: &DebugEvent) -> &[(ProposalKind, RejectReason)] {
    &ev.rejected
}

/// One `Kind reason` line per reject, for humans and CI logs.
#[must_use]
pub fn format_rejects(rejects: &[(ProposalKind, RejectReason)]) -> String {
    let mut lines = Vec::with_capacity(rejects.len());
    for (kind, reason) in rejects {
        lines.push(format!("{kind:?} {reason}"));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use klotho_core::{RejectReason, Tick};
    use klotho_trace::{ProposalKind, TraceDelta};

    use super::*;
    use crate::event::DebugEvent;

    #[test]
    fn inspect_and_format() {
        let mut delta = TraceDelta::empty(Tick(1));
        delta
            .rejects
            .push((ProposalKind::Player, RejectReason::WrongHull));
        delta
            .rejects
            .push((ProposalKind::Infer, RejectReason::UnclaimedAgency));
        let listed = inspect_rejects(&delta);
        assert_eq!(listed.len(), 2);
        let text = format_rejects(listed);
        assert!(text.contains("WrongHull"), "{text}");
        assert!(text.contains("UnclaimedAgency"), "{text}");
        assert!(text.contains("Player"), "{text}");

        let ev = DebugEvent::from_delta(delta, 0);
        assert_eq!(inspect_event(&ev).len(), 2);
        assert!(format_rejects(&[]).is_empty());
    }
}
