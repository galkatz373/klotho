//! 4 ms kernel gate. Shared by space's 64-awake test and `tests/awake64.rs`.

use klotho_core::Budget;

/// How an over-budget 64-awake step is treated.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum BudgetMode {
    /// Print to stderr; do not panic (local debug default).
    Warn,
    /// Panic (CI / release default).
    Fail,
}

impl BudgetMode {
    /// Read `KLOTHO_BUDGET_FAIL`. Unset follows debug/release default.
    #[must_use]
    pub fn from_env() -> Self {
        Self::resolve(
            std::env::var("KLOTHO_BUDGET_FAIL").ok().as_deref(),
            cfg!(debug_assertions),
        )
    }

    /// `Some("1")`/`"true"` → Fail, `Some("0")`/`"false"` → Warn.
    /// Unset: warn when `debug_assertions`, else fail.
    #[must_use]
    pub fn resolve(flag: Option<&str>, debug_assertions: bool) -> Self {
        match flag.map(str::trim) {
            Some(s) if s.eq_ignore_ascii_case("1") || s.eq_ignore_ascii_case("true") => Self::Fail,
            Some(s) if s.eq_ignore_ascii_case("0") || s.eq_ignore_ascii_case("false") => Self::Warn,
            _ if debug_assertions => Self::Warn,
            _ => Self::Fail,
        }
    }

    /// Apply the 4 ms cap (`Budget::HEARTH.us_sim`) to `elapsed_us`.
    pub fn enforce(self, elapsed_us: u128) {
        let cap = u128::from(Budget::HEARTH.us_sim);
        if elapsed_us < cap {
            return;
        }
        match self {
            Self::Warn => {
                eprintln!("klotho-debug: 64-awake step took {elapsed_us} us (cap {cap} us)");
            }
            Self::Fail => {
                panic!("64-awake step is the budget gate, took {elapsed_us} us (cap {cap})");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_flag_and_profile() {
        assert_eq!(BudgetMode::resolve(Some("1"), true), BudgetMode::Fail);
        assert_eq!(BudgetMode::resolve(Some("true"), true), BudgetMode::Fail);
        assert_eq!(BudgetMode::resolve(Some("TRUE"), false), BudgetMode::Fail);
        assert_eq!(BudgetMode::resolve(Some("0"), false), BudgetMode::Warn);
        assert_eq!(BudgetMode::resolve(Some("false"), false), BudgetMode::Warn);
        assert_eq!(BudgetMode::resolve(None, true), BudgetMode::Warn);
        assert_eq!(BudgetMode::resolve(None, false), BudgetMode::Fail);
        assert_eq!(BudgetMode::resolve(Some(" 1 "), true), BudgetMode::Fail);
    }

    #[test]
    fn warn_does_not_panic_over_cap() {
        BudgetMode::Fail.enforce(u128::from(Budget::HEARTH.us_sim) - 1);
        BudgetMode::Warn.enforce(10_000);
    }

    #[test]
    #[cfg(debug_assertions)]
    fn fail_panics_at_cap() {
        let panicked = std::panic::catch_unwind(|| {
            BudgetMode::Fail.enforce(u128::from(Budget::HEARTH.us_sim));
        });
        assert!(panicked.is_err());
    }
}
