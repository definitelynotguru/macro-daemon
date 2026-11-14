//! Descriptive statistics over the per-iteration hold times.
//!
//! `p95` uses the nearest-rank definition: sort the samples, take the value at index
//! `ceil(0.95 * n)` (1-based), i.e. the smallest sample that is at least the 95th
//! percentile. The integration tests pin that choice against hand-computed vectors.

/// Sorted-view summary of a set of hold times in ms.
pub struct Stats {
    pub min: f64,
    pub median: f64,
    pub p95: f64,
    pub max: f64,
}

impl Stats {
    /// Median of an even count is the mean of the middle two (linear definition).
    pub fn new(samples: &[f64]) -> Self {
        assert!(!samples.is_empty(), "stats of an empty sample set");
        let mut sorted: Vec<f64> = samples.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).expect("hold times are finite"));
        let n = sorted.len();
        let median = if n % 2 == 1 {
            sorted[n / 2]
        } else {
            (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
        };
        let rank = ((95 * n + 99) / 100).max(1); // ceil(0.95*n), 1-based, clamped for n=1
        Self {
            min: sorted[0],
            median,
            p95: sorted[rank - 1],
            max: sorted[n - 1],
        }
    }
}
