//! The bounded retries of a preference read that came to nothing: the
//! platform layer's one doubling schedule ([`flui_platform::Backoff`]),
//! counted, so a failure that never heals costs a bounded number of attempts
//! and never a poll. The Windows source, the Linux settings watch and the
//! AppKit and Android sources (`read`) retry on it.

use std::time::Duration;

use flui_platform::Backoff;

/// `limit` retries, the first `first` after the failure and each pause twice
/// the one before, then none until something else asks for a read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Retries {
    schedule: Backoff,
    limit: u32,
}

impl Retries {
    /// The schedule of `limit` retries starting at `first`; its longest pause,
    /// `first` doubled `limit - 1` times, follows from the two.
    ///
    /// # Panics
    /// At compile time for a `limit` above 32, whose last pause no `u32`
    /// factor can name.
    pub(crate) const fn new(first: Duration, limit: u32) -> Self {
        let doublings = limit.saturating_sub(1);
        assert!(
            doublings < 32,
            "BUG: a retry schedule of more than 32 retries"
        );
        Self {
            schedule: Backoff::new(first, first.saturating_mul(1 << doublings)),
            limit,
        }
    }

    /// The pause before retry `spent` (counting from zero); `None` once all
    /// `limit` are spent.
    pub(crate) fn pause(self, spent: u32) -> Option<Duration> {
        (spent < self.limit).then(|| self.schedule.delay(spent))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each schedule doubles its pause and runs out: a failure that never
    /// heals costs a bounded number of attempts.
    #[test]
    fn a_retry_schedule_doubles_and_runs_out() {
        let pauses = |retries: Retries, unit: Duration| {
            (0..10)
                .map_while(|spent| retries.pause(spent))
                .map(|pause| pause.as_nanos() / unit.as_nanos())
                .collect::<Vec<u128>>()
        };
        let second = Duration::from_secs(1);
        assert_eq!(pauses(Retries::new(second, 5), second), [1, 2, 4, 8, 16]);
        assert_eq!(
            pauses(Retries::new(second, 6), second),
            [1, 2, 4, 8, 16, 32]
        );
        let tick = Duration::from_millis(100);
        assert_eq!(pauses(Retries::new(tick, 6), tick), [1, 2, 4, 8, 16, 32]);
        assert_eq!(
            pauses(Retries::new(Duration::MAX, 2), Duration::MAX),
            [1, 1],
            "a pause that cannot double stays at the longest"
        );
        assert_eq!(Retries::new(second, 0).pause(0), None);
    }
}
