//! Memory pressure from the kernel's pressure stall information (PSI).
//!
//! The kernel exposes memory pressure as `/proc/pressure/memory`, a file with a
//! `some` line (the share of time at least one task stalled on memory) and a
//! `full` line (every non-idle task did).
//! Reading the averages would mean polling. Writing a *trigger*
//! (`some 150000 1000000`: wake me when tasks stall 150 ms within any 1 s
//! window) to the same file and polling the descriptor for `POLLPRI` wakes the
//! reader only when pressure is real, so the watcher sleeps otherwise.
//!
//! Two triggers give the two levels: a `some` stall is
//! [`MemoryPressureLevel::Moderate`] (free what is easy to rebuild), a `full` stall
//! [`MemoryPressureLevel::Critical`] (free all but what the frame needs).
//!
//! # Windows the kernel accepts
//!
//! A window is 500 ms to 10 s and a threshold no longer than its window. A
//! process without `CAP_SYS_RESOURCE` may only use a window that is a multiple
//! of 2 s, so each level has a second trigger with a 2 s window and a
//! proportionally longer threshold, tried when the kernel refuses the first.

use std::time::Duration;

use flui_foundation::memory::MemoryPressureLevel;

/// How much of the system waits on memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stall {
    /// At least one task waits.
    Partial,
    /// Every non-idle task waits.
    Full,
}

impl Stall {
    fn word(self) -> &'static str {
        match self {
            Self::Partial => "some",
            Self::Full => "full",
        }
    }

    /// The level this stall is reported as.
    pub(crate) fn level(self) -> MemoryPressureLevel {
        match self {
            Self::Partial => MemoryPressureLevel::Moderate,
            Self::Full => MemoryPressureLevel::Critical,
        }
    }
}

/// The heaviest level any of `fired` asks for, `None` for none.
pub(crate) fn level_of(fired: impl IntoIterator<Item = Stall>) -> Option<MemoryPressureLevel> {
    fired.into_iter().map(Stall::level).max()
}

/// A trigger the kernel accepts: wake when `stall` lasts `threshold` within
/// any `window`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Trigger {
    stall: Stall,
    threshold: Duration,
    window: Duration,
}

/// The kernel's bounds on a trigger's window.
const WINDOW_MIN: Duration = Duration::from_millis(500);
const WINDOW_MAX: Duration = Duration::from_secs(10);
/// The window an unprivileged process may use is a multiple of this.
const UNPRIVILEGED_WINDOW_STEP: Duration = Duration::from_secs(2);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum TriggerError {
    #[error("pressure window {0:?} is outside 500 ms to 10 s")]
    WindowOutOfRange(Duration),
    #[error("pressure threshold {threshold:?} is zero or longer than its window {window:?}")]
    ThresholdOutOfRange {
        threshold: Duration,
        window: Duration,
    },
}

impl Trigger {
    pub(crate) fn new(
        stall: Stall,
        threshold: Duration,
        window: Duration,
    ) -> Result<Self, TriggerError> {
        if !(WINDOW_MIN..=WINDOW_MAX).contains(&window) {
            return Err(TriggerError::WindowOutOfRange(window));
        }
        if threshold.is_zero() || threshold > window {
            return Err(TriggerError::ThresholdOutOfRange { threshold, window });
        }
        Ok(Self {
            stall,
            threshold,
            window,
        })
    }

    pub(crate) fn stall(&self) -> Stall {
        self.stall
    }

    /// Whether the kernel lets a process without `CAP_SYS_RESOURCE` set it.
    #[cfg(test)]
    pub(crate) fn is_unprivileged(&self) -> bool {
        self.window
            .as_micros()
            .is_multiple_of(UNPRIVILEGED_WINDOW_STEP.as_micros())
    }

    /// The text to write to `/proc/pressure/memory`, in microseconds:
    /// `some 150000 1000000`. The kernel takes one write as one request.
    pub(crate) fn request(&self) -> String {
        format!(
            "{} {} {}",
            self.stall.word(),
            self.threshold.as_micros(),
            self.window.as_micros()
        )
    }

    /// The bytes to write to `/proc/pressure/memory`: the request and a NUL.
    /// The kernel overwrites the last byte it is given with its own
    /// terminator, so a write without one loses the last digit of the window
    /// (`some 150000 1000000` arrives as `some 150000 100000`, a threshold
    /// longer than its window, and is refused).
    pub(crate) fn request_bytes(&self) -> Vec<u8> {
        let mut bytes = self.request().into_bytes();
        bytes.push(0);
        bytes
    }
}

/// The triggers to try for `stall`, in the order to try them: the one with a
/// 1 s window, then the one an unprivileged process may set.
pub(crate) fn triggers_for(stall: Stall) -> [Trigger; 2] {
    let threshold = match stall {
        Stall::Partial => Duration::from_millis(150),
        Stall::Full => Duration::from_millis(100),
    };
    let window = Duration::from_secs(1);
    let unprivileged_window = UNPRIVILEGED_WINDOW_STEP;
    // The same share of the window, so the fallback is no more eager.
    let unprivileged_threshold = threshold * 2;
    [
        Trigger::new(stall, threshold, window).expect("BUG: the built-in trigger is in range"),
        Trigger::new(stall, unprivileged_threshold, unprivileged_window)
            .expect("BUG: the built-in fallback trigger is in range"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn psi_trigger_requests() {
        // (stall, level, [first request, fallback request])
        let table = [
            (
                Stall::Partial,
                MemoryPressureLevel::Moderate,
                ["some 150000 1000000", "some 300000 2000000"],
            ),
            (
                Stall::Full,
                MemoryPressureLevel::Critical,
                ["full 100000 1000000", "full 200000 2000000"],
            ),
        ];
        for (stall, level, expected) in table {
            let triggers = triggers_for(stall);
            let requests = triggers.map(|trigger| trigger.request());
            assert_eq!(requests, expected, "{stall:?}");
            for (trigger, text) in triggers.iter().zip(expected) {
                let bytes = trigger.request_bytes();
                assert_eq!(
                    bytes.last(),
                    Some(&0),
                    "{text}: the kernel eats a last byte"
                );
                assert_eq!(&bytes[..bytes.len() - 1], text.as_bytes());
            }
            assert!(
                triggers
                    .iter()
                    .all(|trigger| trigger.stall() == stall && trigger.stall().level() == level)
            );
            assert!(
                !triggers[0].is_unprivileged(),
                "{stall:?}: the 1 s window needs a privilege"
            );
            assert!(
                triggers[1].is_unprivileged(),
                "{stall:?}: the fallback needs none"
            );
        }
    }

    #[test]
    fn psi_trigger_bounds() {
        let ms = Duration::from_millis;
        // (case, threshold, window, accepted)
        let table = [
            ("shortest window", ms(100), ms(500), true),
            ("longest window", ms(100), ms(10_000), true),
            ("threshold equals window", ms(1000), ms(1000), true),
            ("window under 500 ms", ms(100), ms(499), false),
            ("window over 10 s", ms(100), ms(10_001), false),
            ("zero window", ms(0), ms(0), false),
            ("zero threshold", ms(0), ms(1000), false),
            ("threshold over window", ms(1001), ms(1000), false),
        ];
        for (case, threshold, window, accepted) in table {
            assert_eq!(
                Trigger::new(Stall::Partial, threshold, window).is_ok(),
                accepted,
                "{case}"
            );
        }
        assert_eq!(
            Trigger::new(Stall::Full, ms(100), ms(20_000)),
            Err(TriggerError::WindowOutOfRange(ms(20_000)))
        );
        assert_eq!(
            Trigger::new(Stall::Full, ms(0), ms(1000)),
            Err(TriggerError::ThresholdOutOfRange {
                threshold: ms(0),
                window: ms(1000)
            })
        );
    }

    #[test]
    fn psi_unprivileged_window_is_a_multiple_of_two_seconds() {
        let ms = Duration::from_millis;
        for (window, expected) in [
            (ms(1000), false),
            (ms(2000), true),
            (ms(4000), true),
            (ms(3000), false),
            (ms(2500), false),
        ] {
            let trigger = Trigger::new(Stall::Partial, ms(100), window).expect("in range");
            assert_eq!(trigger.is_unprivileged(), expected, "{window:?}");
        }
    }

    #[test]
    fn psi_fired_triggers_make_one_level() {
        use Stall::{Full, Partial};
        // (case, fired stalls, level)
        let table: &[(&str, &[Stall], Option<MemoryPressureLevel>)] = &[
            ("nothing fired", &[], None),
            (
                "some alone",
                &[Partial],
                Some(MemoryPressureLevel::Moderate),
            ),
            ("full alone", &[Full], Some(MemoryPressureLevel::Critical)),
            (
                "both in one poll",
                &[Partial, Full],
                Some(MemoryPressureLevel::Critical),
            ),
            (
                "both, full first",
                &[Full, Partial],
                Some(MemoryPressureLevel::Critical),
            ),
            (
                "the same one twice",
                &[Partial, Partial],
                Some(MemoryPressureLevel::Moderate),
            ),
        ];
        for (case, fired, expected) in table {
            assert_eq!(level_of(fired.iter().copied()), *expected, "{case}");
        }
    }
}
