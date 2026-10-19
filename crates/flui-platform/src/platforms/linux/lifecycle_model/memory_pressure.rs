//! Memory pressure from the kernel's pressure stall information (PSI).
//!
//! `/proc/pressure/memory` has two lines:
//!
//! ```text
//! some avg10=0.00 avg60=0.00 avg300=0.00 total=0
//! full avg10=0.00 avg60=0.00 avg300=0.00 total=0
//! ```
//!
//! `some` is the share of time at least one task stalled on memory, `full`
//! the share in which every non-idle task did; `total` is the stall so far in
//! microseconds. Reading the averages would mean polling. Writing a *trigger*
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

/// A line of `/proc/pressure/memory`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PressureLine {
    pub stall: Stall,
    pub avg10: f64,
    pub avg60: f64,
    pub avg300: f64,
    /// Stall time since boot.
    pub total: Duration,
}

/// The whole of `/proc/pressure/memory`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PressureReport {
    pub some: PressureLine,
    pub full: PressureLine,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub(crate) enum PressureParseError {
    #[error("pressure line is empty")]
    Empty,
    #[error("pressure line starts with `{0}`, not `some` or `full`")]
    UnknownStall(String),
    #[error("pressure line has no `{0}` field")]
    MissingField(&'static str),
    #[error("pressure field `{0}` is not a number")]
    BadField(&'static str),
    #[error("pressure file has no `{0}` line")]
    MissingLine(&'static str),
}

impl PressureLine {
    /// Parse one line. Fields it does not know are skipped, as the kernel may
    /// add some; a field it needs that is missing or not a finite
    /// non-negative number is an error.
    pub(crate) fn parse(line: &str) -> Result<Self, PressureParseError> {
        let mut words = line.split_whitespace();
        let stall = match words.next().ok_or(PressureParseError::Empty)? {
            "some" => Stall::Partial,
            "full" => Stall::Full,
            other => return Err(PressureParseError::UnknownStall(other.to_owned())),
        };
        let (mut avg10, mut avg60, mut avg300, mut total) = (None, None, None, None);
        for word in words {
            let Some((key, value)) = word.split_once('=') else {
                continue;
            };
            match key {
                "avg10" => avg10 = Some(average(value, "avg10")?),
                "avg60" => avg60 = Some(average(value, "avg60")?),
                "avg300" => avg300 = Some(average(value, "avg300")?),
                "total" => {
                    let micros = value
                        .parse::<u64>()
                        .map_err(|_| PressureParseError::BadField("total"))?;
                    total = Some(Duration::from_micros(micros));
                }
                _ => {}
            }
        }
        Ok(Self {
            stall,
            avg10: avg10.ok_or(PressureParseError::MissingField("avg10"))?,
            avg60: avg60.ok_or(PressureParseError::MissingField("avg60"))?,
            avg300: avg300.ok_or(PressureParseError::MissingField("avg300"))?,
            total: total.ok_or(PressureParseError::MissingField("total"))?,
        })
    }
}

fn average(value: &str, field: &'static str) -> Result<f64, PressureParseError> {
    value
        .parse::<f64>()
        .ok()
        .filter(|average| average.is_finite() && *average >= 0.0)
        .ok_or(PressureParseError::BadField(field))
}

impl PressureReport {
    /// Parse the file's text. Both lines must be there: a kernel that reports
    /// only `some` for memory predates the triggers this module writes.
    pub(crate) fn parse(text: &str) -> Result<Self, PressureParseError> {
        let (mut some, mut full) = (None, None);
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let line = PressureLine::parse(line)?;
            match line.stall {
                Stall::Partial => some = Some(line),
                Stall::Full => full = Some(line),
            }
        }
        Ok(Self {
            some: some.ok_or(PressureParseError::MissingLine("some"))?,
            full: full.ok_or(PressureParseError::MissingLine("full"))?,
        })
    }
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

    /// The level a wake from this trigger reports.
    pub(crate) fn level(&self) -> MemoryPressureLevel {
        self.stall.level()
    }

    /// Whether the kernel lets a process without `CAP_SYS_RESOURCE` set it.
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

    const IDLE: &str = "some avg10=0.00 avg60=0.00 avg300=0.00 total=0\n\
                        full avg10=0.00 avg60=0.00 avg300=0.00 total=0\n";

    #[test]
    fn psi_line_parse() {
        // (case, line, parsed)
        let ok: &[(&str, &str, PressureLine)] = &[
            (
                "idle some",
                "some avg10=0.00 avg60=0.00 avg300=0.00 total=0",
                PressureLine {
                    stall: Stall::Partial,
                    avg10: 0.0,
                    avg60: 0.0,
                    avg300: 0.0,
                    total: Duration::ZERO,
                },
            ),
            (
                "busy full",
                "full avg10=12.50 avg60=3.07 avg300=0.41 total=48213377",
                PressureLine {
                    stall: Stall::Full,
                    avg10: 12.5,
                    avg60: 3.07,
                    avg300: 0.41,
                    total: Duration::from_micros(48_213_377),
                },
            ),
            (
                "fields in another order",
                "some total=7 avg300=3 avg60=2 avg10=1",
                PressureLine {
                    stall: Stall::Partial,
                    avg10: 1.0,
                    avg60: 2.0,
                    avg300: 3.0,
                    total: Duration::from_micros(7),
                },
            ),
            (
                "unknown field and trailing blanks",
                "  some avg10=0.10 avg60=0.20 avg300=0.30 total=9 avg1800=0.4  \n",
                PressureLine {
                    stall: Stall::Partial,
                    avg10: 0.1,
                    avg60: 0.2,
                    avg300: 0.3,
                    total: Duration::from_micros(9),
                },
            ),
            (
                "total past u32",
                "some avg10=0 avg60=0 avg300=0 total=18446744073709551615",
                PressureLine {
                    stall: Stall::Partial,
                    avg10: 0.0,
                    avg60: 0.0,
                    avg300: 0.0,
                    total: Duration::from_micros(u64::MAX),
                },
            ),
        ];
        for (case, line, expected) in ok {
            assert_eq!(PressureLine::parse(line), Ok(*expected), "{case}");
        }

        // (case, line, error)
        let bad: &[(&str, &str, PressureParseError)] = &[
            ("empty", "", PressureParseError::Empty),
            ("blank", "   \t", PressureParseError::Empty),
            (
                "cpu-style word",
                "cpu avg10=0 avg60=0 avg300=0 total=0",
                PressureParseError::UnknownStall("cpu".into()),
            ),
            (
                "no total",
                "some avg10=0 avg60=0 avg300=0",
                PressureParseError::MissingField("total"),
            ),
            (
                "no averages",
                "some total=0",
                PressureParseError::MissingField("avg10"),
            ),
            (
                "text total",
                "some avg10=0 avg60=0 avg300=0 total=lots",
                PressureParseError::BadField("total"),
            ),
            (
                "negative total",
                "some avg10=0 avg60=0 avg300=0 total=-1",
                PressureParseError::BadField("total"),
            ),
            (
                "total past u64",
                "some avg10=0 avg60=0 avg300=0 total=18446744073709551616",
                PressureParseError::BadField("total"),
            ),
            (
                "nan average",
                "some avg10=nan avg60=0 avg300=0 total=0",
                PressureParseError::BadField("avg10"),
            ),
            (
                "infinite average",
                "some avg10=0 avg60=inf avg300=0 total=0",
                PressureParseError::BadField("avg60"),
            ),
            (
                "negative average",
                "some avg10=0 avg60=0 avg300=-0.5 total=0",
                PressureParseError::BadField("avg300"),
            ),
            (
                "empty average",
                "some avg10= avg60=0 avg300=0 total=0",
                PressureParseError::BadField("avg10"),
            ),
        ];
        for (case, line, expected) in bad {
            assert_eq!(PressureLine::parse(line), Err(expected.clone()), "{case}");
        }
    }

    #[test]
    fn psi_file_parse() {
        let report = PressureReport::parse(IDLE).expect("an idle file parses");
        assert_eq!(report.some.stall, Stall::Partial);
        assert_eq!(report.full.stall, Stall::Full);

        // Lines in either order, and blank lines between them.
        let swapped = "full avg10=1 avg60=1 avg300=1 total=5\n\n\
                       some avg10=2 avg60=2 avg300=2 total=6\n";
        let report = PressureReport::parse(swapped).expect("order does not matter");
        assert_eq!(report.some.total, Duration::from_micros(6));
        assert_eq!(report.full.total, Duration::from_micros(5));

        // (case, text, error)
        let bad: &[(&str, &str, PressureParseError)] = &[
            ("empty file", "", PressureParseError::MissingLine("some")),
            (
                "kernel before the full line",
                "some avg10=0 avg60=0 avg300=0 total=0\n",
                PressureParseError::MissingLine("full"),
            ),
            (
                "a bad line fails the file",
                "some avg10=0 avg60=0 avg300=0 total=x\nfull avg10=0 avg60=0 avg300=0 total=0\n",
                PressureParseError::BadField("total"),
            ),
        ];
        for (case, text, expected) in bad {
            assert_eq!(PressureReport::parse(text), Err(expected.clone()), "{case}");
        }
    }

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
                    .all(|trigger| trigger.stall() == stall && trigger.level() == level)
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
