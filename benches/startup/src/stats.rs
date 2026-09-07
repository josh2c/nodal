//! Order statistics over a set of timings. Pure: no clock, no process, no IO.

use std::time::Duration;

/// The timings of one measurement set, sorted from fastest to slowest.
pub struct Sorted(Vec<Duration>);

impl Sorted {
    /// Sort the timings. The caller keeps no order of its own, so this consumes them.
    pub fn new(mut timings: Vec<Duration>) -> Self {
        timings.sort_unstable();
        Self(timings)
    }

    /// The middle value. For an even count, the mean of the two middle values.
    ///
    /// The median is the number the gate compares, because one slow run on a shared
    /// runner must not move it. A mean would move; the median does not.
    pub fn median(&self) -> Duration {
        let count = self.0.len();
        if count == 0 {
            return Duration::ZERO;
        }
        let upper = self.at(count / 2);
        if count % 2 == 1 {
            return upper;
        }
        (self.at(count / 2 - 1) + upper) / 2
    }

    /// The value at the given percentile, by nearest rank.
    pub fn percentile(&self, percentile: usize) -> Duration {
        let count = self.0.len();
        if count == 0 {
            return Duration::ZERO;
        }
        let rank = percentile.saturating_mul(count).div_ceil(100).max(1);
        self.at(rank - 1)
    }

    /// The fastest run.
    pub fn min(&self) -> Duration {
        self.at(0)
    }

    /// The slowest run.
    pub fn max(&self) -> Duration {
        self.at(self.0.len().saturating_sub(1))
    }

    /// How many runs the set holds.
    pub fn count(&self) -> usize {
        self.0.len()
    }

    /// The timing at an index, or zero when the set is empty.
    fn at(&self, index: usize) -> Duration {
        self.0.get(index).copied().unwrap_or(Duration::ZERO)
    }
}

/// Format a duration in milliseconds, to three decimal places.
pub fn milliseconds(duration: Duration) -> String {
    format!("{:.3}", duration.as_secs_f64() * 1_000.0)
}

#[cfg(test)]
mod tests {
    use super::{Sorted, milliseconds};
    use std::time::Duration;

    fn set(values: &[u64]) -> Sorted {
        Sorted::new(values.iter().map(|&ms| Duration::from_millis(ms)).collect())
    }

    #[test]
    fn median_of_an_odd_count_is_the_middle_value() {
        assert_eq!(set(&[5, 1, 3]).median(), Duration::from_millis(3));
    }

    #[test]
    fn median_of_an_even_count_is_the_mean_of_the_two_middle_values() {
        assert_eq!(set(&[1, 2, 4, 9]).median(), Duration::from_millis(3));
    }

    #[test]
    fn one_slow_run_does_not_move_the_median() {
        assert_eq!(set(&[1, 1, 1, 1, 900]).median(), Duration::from_millis(1));
    }

    #[test]
    fn percentiles_take_the_nearest_rank() {
        let values: Vec<u64> = (1..=100).collect();
        let sorted = set(&values);
        assert_eq!(sorted.percentile(50), Duration::from_millis(50));
        assert_eq!(sorted.percentile(95), Duration::from_millis(95));
        assert_eq!(sorted.percentile(100), Duration::from_millis(100));
    }

    #[test]
    fn an_empty_set_reports_zero_and_never_panics() {
        let empty = Sorted::new(Vec::new());
        assert_eq!(empty.count(), 0);
        assert_eq!(empty.median(), Duration::ZERO);
        assert_eq!(empty.percentile(95), Duration::ZERO);
        assert_eq!(empty.min(), Duration::ZERO);
        assert_eq!(empty.max(), Duration::ZERO);
    }

    #[test]
    fn milliseconds_keeps_three_decimal_places() {
        assert_eq!(milliseconds(Duration::from_micros(1_234)), "1.234");
    }
}
