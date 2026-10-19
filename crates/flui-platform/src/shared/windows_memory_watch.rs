//! When the system's low-memory event becomes a notice for the owner.
//!
//! `CreateMemoryResourceNotification` gives an event that stays signaled for as
//! long as memory is low: it is a level, not an edge. A wait registered on it
//! without care fires again at once, forever. The watch therefore waits
//! once, reports once, stays quiet for a hold interval whatever the event does,
//! and only then waits again, so memory that stays low is reported at most once
//! per interval. A signal that lands while a report is already owed (the pool
//! thread had one in flight) joins it.
//!
//! The state is a plain value so every host runs the rules; the backend keeps
//! it behind a lock the pool thread and the owner share.

use std::time::Duration;

/// How long after a report the watch stays deaf before it waits again.
pub(crate) const HOLD_INTERVAL: Duration = Duration::from_secs(30);

/// Where the watch is in its cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// A wait is registered on the event.
    Armed,
    /// The event fired; the owner has not yet taken the report.
    Owed,
    /// The owner took the report at `since`; the watch is deaf until the hold
    /// interval ends.
    Holding { since: Duration },
}

/// The cycle of one low-memory watch. Times are a monotonic clock's readings.
#[derive(Debug)]
pub(crate) struct MemoryWatch {
    phase: Phase,
}

impl MemoryWatch {
    /// A watch whose wait is registered.
    pub(crate) const fn armed() -> Self {
        Self {
            phase: Phase::Armed,
        }
    }

    /// The event fired. Returns whether the owner must be woken: `false` when
    /// a report is already owed or the watch is holding, because the signal
    /// joins what is pending or is the same low-memory episode.
    pub(crate) fn signaled(&mut self) -> bool {
        match self.phase {
            Phase::Armed => {
                self.phase = Phase::Owed;
                true
            }
            Phase::Owed | Phase::Holding { .. } => false,
        }
    }

    /// The owner takes what is owed at `now`. Returns whether a report is to
    /// be delivered; the hold starts with it.
    pub(crate) fn take(&mut self, now: Duration) -> bool {
        match self.phase {
            Phase::Owed => {
                self.phase = Phase::Holding { since: now };
                true
            }
            Phase::Armed | Phase::Holding { .. } => false,
        }
    }

    /// Whether the hold is over at `now` and the wait should be registered
    /// again. Answers `true` once per hold; a clock that reads earlier than the
    /// start of the hold counts as no time passed.
    pub(crate) fn rearm(&mut self, now: Duration) -> bool {
        match self.phase {
            Phase::Holding { since } if now.saturating_sub(since) >= HOLD_INTERVAL => {
                self.phase = Phase::Armed;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{HOLD_INTERVAL, MemoryWatch};

    const START: Duration = Duration::from_secs(1000);

    fn a_signal_owes_one_report_that_the_owner_takes() {
        let mut watch = MemoryWatch::armed();
        assert!(watch.signaled(), "the first signal wakes the owner");
        assert!(watch.take(START));
        assert!(!watch.take(START), "a report is delivered once");
    }

    fn signals_while_a_report_is_owed_join_it() {
        let mut watch = MemoryWatch::armed();
        assert!(watch.signaled());
        for _ in 0..5 {
            assert!(!watch.signaled(), "the owner is already woken");
        }
        assert!(watch.take(START));
        assert!(!watch.take(START), "five signals, one report");
    }

    fn a_signal_during_the_hold_is_the_same_episode() {
        let mut watch = MemoryWatch::armed();
        assert!(watch.signaled());
        assert!(watch.take(START));
        // The pool thread had a callback in flight when the owner took the report.
        assert!(!watch.signaled());
        assert!(!watch.take(START));
        assert!(
            !watch.rearm(START + HOLD_INTERVAL.saturating_sub(Duration::from_millis(1))),
            "armed too early"
        );
    }

    fn the_wait_is_registered_again_only_after_the_hold() {
        let mut watch = MemoryWatch::armed();
        assert!(watch.signaled());
        assert!(watch.take(START));
        assert!(!watch.rearm(START));
        assert!(watch.rearm(START + HOLD_INTERVAL));
        assert!(!watch.rearm(START + HOLD_INTERVAL * 2), "once per hold");
        // Armed again: a new episode is a new report.
        assert!(watch.signaled());
        assert!(watch.take(START + HOLD_INTERVAL * 2));
    }

    fn the_owner_taking_nothing_changes_nothing() {
        let mut watch = MemoryWatch::armed();
        assert!(!watch.take(START));
        assert!(!watch.rearm(START + HOLD_INTERVAL * 10));
        assert!(watch.signaled(), "an armed watch is still armed");
    }

    fn a_clock_that_reads_earlier_counts_as_no_time_passed() {
        let mut watch = MemoryWatch::armed();
        assert!(watch.signaled());
        assert!(watch.take(START));
        assert!(!watch.rearm(Duration::ZERO));
    }

    #[test]
    fn low_memory_reports() {
        crate::table_test::run_table(
            "low_memory_reports",
            &[
                (
                    "a_signal_owes_one_report_that_the_owner_takes",
                    a_signal_owes_one_report_that_the_owner_takes as fn(),
                ),
                (
                    "signals_while_a_report_is_owed_join_it",
                    signals_while_a_report_is_owed_join_it as fn(),
                ),
                (
                    "a_signal_during_the_hold_is_the_same_episode",
                    a_signal_during_the_hold_is_the_same_episode as fn(),
                ),
                (
                    "the_wait_is_registered_again_only_after_the_hold",
                    the_wait_is_registered_again_only_after_the_hold as fn(),
                ),
                (
                    "the_owner_taking_nothing_changes_nothing",
                    the_owner_taking_nothing_changes_nothing as fn(),
                ),
                (
                    "a_clock_that_reads_earlier_counts_as_no_time_passed",
                    a_clock_that_reads_earlier_counts_as_no_time_passed as fn(),
                ),
            ],
        );
    }
}
