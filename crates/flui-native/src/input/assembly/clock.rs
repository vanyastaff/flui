//! Native reading clocks rebased onto the shared event epoch (ADR-0207 §2).
//!
//! A reading names the clock it was measured on; [`NativeClock`] holds the
//! anchors that place each clock on the epoch. The anchors are the header of a
//! native recording (ADR-0215 §4), so a recorded reading replays to the same
//! [`EventTime`] on any host.

use std::num::NonZeroU64;
use std::time::Duration;

use flui_platform::EventTime;

/// A `QueryPerformanceCounter` reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct QpcTicks(pub u64);

/// When the OS says a reading was measured, on the clock the OS used.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum NativeTime {
    /// The low 32 bits of a millisecond uptime (Win32 `dwTime`,
    /// `GetMessageTime`), with the full uptime read when the reading arrived.
    /// The tick is placed at the nearest signed offset from `observed_ms`, so
    /// it survives the 49.7-day wrap.
    TickMillis {
        /// The wrapping tick.
        tick: u32,
        /// The 64-bit uptime, in milliseconds, read on arrival.
        observed_ms: u64,
    },
    /// Seconds of system uptime (AppKit and UIKit timestamps).
    UptimeSeconds(f64),
    /// Already on the shared event epoch.
    Epoch(EventTime),
}

/// The time fields one native reading carries: a coarse stamp every reading
/// has and the high-resolution counter some devices add.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReadingTime {
    /// The stamp every reading has.
    pub coarse: NativeTime,
    /// The performance count, when the device reports one.
    pub counter: Option<QpcTicks>,
}

impl ReadingTime {
    /// A reading stamped only on `coarse`.
    #[must_use]
    pub const fn coarse(coarse: NativeTime) -> Self {
        Self {
            coarse,
            counter: None,
        }
    }

    /// A reading already on the event epoch.
    #[must_use]
    pub const fn epoch(time: EventTime) -> Self {
        Self::coarse(NativeTime::Epoch(time))
    }
}

/// The performance counter read at the clock's epoch, with its frequency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CounterAnchor {
    /// The counter value at the epoch instant.
    pub count: QpcTicks,
    /// Counts per second.
    pub frequency: NonZeroU64,
}

impl CounterAnchor {
    fn nanos(self, ticks: u64) -> u64 {
        counter_nanos(ticks, self.frequency)
    }
}

/// `ticks` of a counter running at `frequency` per second, in nanoseconds:
/// truncated, computed in `u128` so no intermediate overflows, and saturated
/// at `u64::MAX` (some 584 years) for a span past it.
#[must_use]
pub(crate) fn counter_nanos(ticks: u64, frequency: NonZeroU64) -> u64 {
    let nanos = u128::from(ticks) * 1_000_000_000 / u128::from(frequency.get());
    u64::try_from(nanos).unwrap_or(u64::MAX)
}

/// Which clock stamps a sequence: chosen per device when its sequence starts
/// and kept until that sequence ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClockChoice {
    /// The performance counter.
    Counter,
    /// The reading's coarse stamp.
    Coarse,
}

/// The anchors that place every native clock on the shared event epoch.
///
/// Read once, back to back, when a backend's input owner starts; the
/// recording header carries the same values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeClock {
    epoch: EventTime,
    uptime: Duration,
    counter: Option<CounterAnchor>,
}

impl NativeClock {
    /// A clock whose epoch instant was `epoch`, when system uptime was
    /// `uptime` and the performance counter, if any, was `counter`.
    #[must_use]
    pub const fn new(epoch: EventTime, uptime: Duration, counter: Option<CounterAnchor>) -> Self {
        Self {
            epoch,
            uptime,
            counter,
        }
    }

    /// A clock for readings already on the event epoch.
    #[must_use]
    pub const fn epoch_only() -> Self {
        Self::new(EventTime::from_nanos(0), Duration::ZERO, None)
    }

    /// The epoch instant, the uptime and the performance counter it was
    /// anchored at: the header of a captured recording.
    #[cfg(all(debug_assertions, feature = "native-capture"))]
    pub(crate) const fn anchors(&self) -> (EventTime, Duration, Option<CounterAnchor>) {
        (self.epoch, self.uptime, self.counter)
    }

    /// The performance counter anchor, when the host has one.
    #[must_use]
    pub const fn counter(&self) -> Option<CounterAnchor> {
        self.counter
    }

    /// Whether a reading at `time` can be stamped on `choice`.
    #[must_use]
    pub const fn supports(&self, time: ReadingTime, choice: ClockChoice) -> bool {
        match choice {
            ClockChoice::Counter => self.counter.is_some() && time.counter.is_some(),
            ClockChoice::Coarse => true,
        }
    }

    /// `time` on the event epoch, read from the clock `choice` names, or from
    /// the coarse stamp when the reading lacks that clock. `None` when the
    /// coarse stamp is not a finite time.
    #[must_use]
    pub fn stamp(&self, time: ReadingTime, choice: ClockChoice) -> Option<EventTime> {
        if choice == ClockChoice::Counter
            && let (Some(anchor), Some(count)) = (self.counter, time.counter)
        {
            let nanos = if count >= anchor.count {
                self.epoch
                    .as_nanos()
                    .saturating_add(anchor.nanos(count.0 - anchor.count.0))
            } else {
                self.epoch
                    .as_nanos()
                    .saturating_sub(anchor.nanos(anchor.count.0 - count.0))
            };
            return Some(EventTime::from_nanos(nanos));
        }
        self.coarse(time.coarse)
    }

    fn coarse(&self, time: NativeTime) -> Option<EventTime> {
        match time {
            NativeTime::Epoch(time) => Some(time),
            NativeTime::TickMillis { tick, observed_ms } => {
                // Extend the nearest signed offset, including across the wrap.
                // The coarse reference can lag a native tick by a few
                // milliseconds; an unsigned age would mistake that lead for a
                // full wrap. Ticks half a wrap or more from the reference are
                // ambiguous and resolve to the nearer half.
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "the tick is the low 32 bits of the uptime"
                )]
                let offset_ms = tick.wrapping_sub(observed_ms as u32).cast_signed();
                let sample_ms = observed_ms.saturating_add_signed(i64::from(offset_ms));
                Some(self.after_uptime(Duration::from_millis(sample_ms)))
            }
            NativeTime::UptimeSeconds(seconds) => {
                if !seconds.is_finite() {
                    return None;
                }
                let sample = Duration::try_from_secs_f64(seconds.max(0.0)).ok()?;
                Some(self.after_uptime(sample))
            }
        }
    }

    fn after_uptime(&self, sample: Duration) -> EventTime {
        let nanos = |duration: Duration| u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX);
        EventTime::from_nanos(if sample >= self.uptime {
            self.epoch
                .as_nanos()
                .saturating_add(nanos(sample.saturating_sub(self.uptime)))
        } else {
            self.epoch
                .as_nanos()
                .saturating_sub(nanos(self.uptime.saturating_sub(sample)))
        })
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use super::counter_nanos;

    /// Counter ticks become nanoseconds with no intermediate overflow, and a
    /// span past `u64::MAX` nanoseconds saturates rather than wraps.
    #[test]
    fn counter_ticks_become_nanoseconds_at_the_extremes() {
        let hz = |frequency: u64| NonZeroU64::new(frequency).expect("a nonzero frequency");
        let cases = [
            ("nothing elapsed", 0, hz(10_000_000), 0),
            ("one tick at 10 MHz", 1, hz(10_000_000), 100),
            (
                "a second at 10 MHz",
                10_000_000,
                hz(10_000_000),
                1_000_000_000,
            ),
            ("a tick shorter than a nanosecond", 1, hz(3_000_000_000), 0),
            ("one tick of the fastest counter", 1, NonZeroU64::MAX, 0),
            (
                "the whole counter at the fastest frequency",
                u64::MAX,
                NonZeroU64::MAX,
                1_000_000_000,
            ),
            (
                "a product past u64 whose quotient fits",
                u64::MAX,
                hz(2_000_000_000),
                u64::MAX / 2,
            ),
            ("past u64 nanoseconds at 1 Hz", u64::MAX, hz(1), u64::MAX),
        ];
        for (case, ticks, frequency, nanos) in cases {
            assert_eq!(counter_nanos(ticks, frequency), nanos, "{case}");
        }
    }
}
