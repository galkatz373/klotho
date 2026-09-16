//! Disposable bounded-workload telemetry, outside Canon/Trace/Projection.
use crate::SolveTimings;

/// Microsecond distribution over a single physical stage.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct StageQuantiles {
    /// Median measured sample.
    pub p50_us: u32,
    /// 95th percentile measured sample.
    pub p95_us: u32,
    /// 99th percentile measured sample.
    pub p99_us: u32,
    /// Largest measured sample.
    pub max_us: u32,
}

/// Per-stage distribution and workload maxima for a bounded run.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct PerformanceSummary {
    /// Number of successful island solves measured.
    pub samples: usize,
    /// Candidate/body collection.
    pub broad: StageQuantiles,
    /// Character traversal preparation.
    pub character: StageQuantiles,
    /// Vehicle wheel-query preparation.
    pub vehicle: StageQuantiles,
    /// Contact generation.
    pub narrow: StageQuantiles,
    /// Constraint and contact solve.
    pub constraint: StageQuantiles,
    /// Proposal quantization and semantic contact sampling.
    pub encode: StageQuantiles,
    /// Largest island body count observed.
    pub max_bodies: u32,
    /// Largest partition membership observed.
    pub max_members: u32,
    /// Largest contact set observed.
    pub max_contacts: u32,
    /// Largest canonical constraint count observed.
    pub max_constraints: u32,
}

fn quantiles(samples: &[SolveTimings], field: impl Fn(&SolveTimings) -> u32) -> StageQuantiles {
    let mut values: Vec<_> = samples.iter().map(field).collect();
    values.sort_unstable();
    let at = |n: usize| values[(values.len() - 1) * n / 100];
    StageQuantiles {
        p50_us: at(50),
        p95_us: at(95),
        p99_us: at(99),
        max_us: *values.last().expect("nonempty samples"),
    }
}

/// Summarize measured stage samples. Empty input has no performance claim.
#[must_use]
pub fn summarize_timings(samples: &[SolveTimings]) -> Option<PerformanceSummary> {
    if samples.is_empty() {
        return None;
    }
    Some(PerformanceSummary {
        samples: samples.len(),
        broad: quantiles(samples, |s| s.broad_us),
        character: quantiles(samples, |s| s.character_us),
        vehicle: quantiles(samples, |s| s.vehicle_us),
        narrow: quantiles(samples, |s| s.narrow_us),
        constraint: quantiles(samples, |s| s.constraint_us),
        encode: quantiles(samples, |s| s.encode_us),
        max_bodies: samples.iter().map(|s| s.bodies).max().unwrap_or(0),
        max_members: samples.iter().map(|s| s.members).max().unwrap_or(0),
        max_contacts: samples.iter().map(|s| s.max_contacts).max().unwrap_or(0),
        max_constraints: samples.iter().map(|s| s.constraints).max().unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quantiles_are_order_independent_and_report_max_workload() {
        let mut samples: Vec<_> = (0..100)
            .map(|n| SolveTimings {
                broad_us: n,
                bodies: n,
                members: n + 1,
                max_contacts: n / 2,
                ..SolveTimings::default()
            })
            .collect();
        samples.reverse();
        let report = summarize_timings(&samples).unwrap();
        assert_eq!(
            (
                report.broad.p50_us,
                report.broad.p95_us,
                report.broad.p99_us,
                report.broad.max_us
            ),
            (49, 94, 98, 99)
        );
        assert_eq!((report.max_bodies, report.max_contacts), (99, 49));
        assert_eq!(report.max_members, 100);
        assert!(summarize_timings(&[]).is_none());
    }
}
