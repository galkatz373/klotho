//! Journey and budget diagnostics. Journey execution is KAI-06; the envelope is KAI-04.

use klotho_core::Budget;
use klotho_ir::{Diagnostic, diagnose_budget, diagnose_journey};

/// Unreachable journey: last reachable semantic state and the blocked affordance.
#[must_use]
pub fn diagnose_unreachable_journey(
    journey: &str,
    last_state: &str,
    blocked_affordance: &str,
) -> Diagnostic {
    diagnose_journey(
        journey,
        last_state,
        blocked_affordance,
        format!("Journey({last_state}->{blocked_affordance})"),
    )
}

/// Budget miss. `dominant` is most-expensive first. `us_sim` is telemetry, not admission.
#[must_use]
pub fn diagnose_budget_miss(
    subject: &str,
    dominant: &[&str],
    used: u32,
    budget: Budget,
) -> Diagnostic {
    diagnose_budget(
        subject,
        dominant,
        used,
        budget.us_sim,
        format!("Budget({used}>{})", budget.us_sim),
    )
}

#[cfg(test)]
mod tests {
    use klotho_ir::DiagnosticCode;

    use super::*;

    #[test]
    fn journey_points_at_anchor_and_blocked_affordance() {
        let d = diagnose_unreachable_journey("open-door", "has-brass-key", "Openable");
        assert_eq!(d.code.0, DiagnosticCode::JOURNEY);
        assert_eq!(d.message, "Journey(has-brass-key->Openable)");
        assert!(d.points_to_anchor());
        assert!(!d.legal_repairs.is_empty());
        assert!(d.related.iter().any(|a| a != &klotho_ir::AnchorId::ZERO));
    }

    #[test]
    fn budget_names_dominant_loci() {
        let d = diagnose_budget_miss(
            "aaa_adventure",
            &["hearth-fire", "crowd"],
            9_000,
            Budget::AAA_ADVENTURE,
        );
        assert_eq!(d.code.0, DiagnosticCode::BUDGET);
        assert!(d.points_to_anchor());
        match d.witness {
            Some(klotho_ir::Counterexample::Budget {
                dominant,
                used,
                cap,
                estimated_savings,
            }) => {
                assert_eq!(dominant, ["hearth-fire", "crowd"]);
                assert_eq!(used, 9_000);
                assert_eq!(cap, Budget::AAA_ADVENTURE.us_sim);
                assert_eq!(estimated_savings, 1_000);
            }
            other => panic!("{other:?}"),
        }
    }
}
