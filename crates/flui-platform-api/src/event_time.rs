//! The timestamp every input event carries.

use core::time::Duration;

/// When the platform observed an input event, on the process's monotonic event timeline.
///
/// The value is nanoseconds since one epoch shared by every backend in the process, so two
/// events are comparable whichever device or window produced them; values from different
/// processes are not. A backend stamps the time the operating system reports for the event
/// (`GetMessageTime`, `NSEvent.timestamp`, `MotionEvent.getEventTimeNanos`, the DOM
/// `timeStamp`), rebased onto that epoch, and stamps its own reading of the clock only where
/// the platform reports none. The timeline never runs backwards, so velocity and multi-tap
/// timing can subtract two stamps; [`saturating_duration_since`](Self::saturating_duration_since)
/// still clamps a pair a backend delivered out of order rather than underflowing.
///
/// # Examples
///
/// ```
/// use core::time::Duration;
/// use flui_platform_api::EventTime;
///
/// let down = EventTime::from_nanos(1_000_000);
/// let up = EventTime::from_nanos(9_000_000);
/// assert_eq!(up.saturating_duration_since(down), Duration::from_millis(8));
/// assert_eq!(down.saturating_duration_since(up), Duration::ZERO);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventTime(u64);

impl EventTime {
    /// The stamp `nanos` nanoseconds after the process's event epoch.
    #[must_use]
    pub const fn from_nanos(nanos: u64) -> Self {
        Self(nanos)
    }

    /// Nanoseconds since the process's event epoch.
    #[must_use]
    pub const fn as_nanos(self) -> u64 {
        self.0
    }

    /// The time from `earlier` to `self`, or zero when `earlier` is the later stamp.
    #[must_use]
    pub const fn saturating_duration_since(self, earlier: Self) -> Duration {
        Duration::from_nanos(self.0.saturating_sub(earlier.0))
    }
}
