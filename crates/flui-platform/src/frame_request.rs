//! What a host reports when it asks for a frame, and what it learned about the frames it showed.

use core::time::Duration;

use crate::EventTime;

/// A request for one frame, on the event timeline of [`EventTime`].
///
/// A backend fills what its platform reports and nothing it would have to invent (ADR-0254):
/// [`vsync`](Self::vsync) is the platform's frame timestamp rebased onto the event epoch, or the
/// backend's own reading of that clock when the request was admitted where the platform reports
/// none, and [`target_present`](Self::target_present) is `None` unless the platform names when
/// this frame will be shown. Neither is a prediction. [`presented`](Self::presented) is the
/// newest earlier frame of the window that the platform reported on the screen since the window's
/// last request, so a request and the input before it are measured on one timeline.
///
/// # Examples
///
/// ```
/// use flui_platform::{EventTime, FramePresented, FrameRequest};
///
/// let vsync = EventTime::from_nanos(16_000_000);
/// let shown = EventTime::from_nanos(33_000_000);
/// let request = FrameRequest::new(vsync).with_target_present(shown);
/// assert_eq!(request.vsync, vsync);
/// assert_eq!(request.target_present, Some(shown));
/// assert_eq!(FrameRequest::new(vsync).target_present, None);
///
/// let earlier = FramePresented::new(EventTime::from_nanos(9_000_000));
/// let request = FrameRequest::new(vsync).with_presented(earlier);
/// assert_eq!(request.presented, Some(earlier));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct FrameRequest {
    /// When the frame's vsync happened.
    pub vsync: EventTime,
    /// When the platform says this frame will be shown, if it says.
    pub target_present: Option<EventTime>,
    /// The newest earlier frame the platform reported shown since the window's last request, if
    /// it reported one. A window whose capabilities say it reports no presentation
    /// ([`WindowCapabilities::presentation_feedback`](crate::WindowCapabilities::presentation_feedback))
    /// never fills it.
    pub presented: Option<FramePresented>,
}

impl FrameRequest {
    /// A request whose vsync is `vsync` and whose present time is unknown.
    #[must_use]
    pub const fn new(vsync: EventTime) -> Self {
        Self {
            vsync,
            target_present: None,
            presented: None,
        }
    }

    /// The same request with the platform's target present time.
    #[must_use]
    pub const fn with_target_present(mut self, target_present: EventTime) -> Self {
        self.target_present = Some(target_present);
        self
    }

    /// The same request carrying the platform's report of an earlier frame shown.
    #[must_use]
    pub const fn with_presented(mut self, presented: FramePresented) -> Self {
        self.presented = Some(presented);
        self
    }
}

impl From<EventTime> for FrameRequest {
    /// A request that knows only its vsync.
    fn from(vsync: EventTime) -> Self {
        Self::new(vsync)
    }
}

/// The platform's report that a frame presented into a window reached the screen.
///
/// [`at`](Self::at) is the instant the display started showing the frame (the refresh it was
/// latched for), as the operating system measured it, rebased onto the event epoch: it compares
/// with an input event's [`EventTime`] without conversion, so the time from an input to the frame
/// that showed its effect is a subtraction. It is never a prediction: a platform that only names
/// when a frame will be shown fills [`FrameRequest::target_present`] instead.
///
/// # Examples
///
/// ```
/// use core::time::Duration;
/// use flui_platform::{EventTime, FramePresented};
///
/// let tap = EventTime::from_nanos(1_000_000);
/// let shown = FramePresented::new(EventTime::from_nanos(25_000_000))
///     .with_refresh(Duration::from_micros(16_667));
/// assert_eq!(shown.at.saturating_duration_since(tap), Duration::from_millis(24));
/// assert_eq!(shown.refresh, Some(Duration::from_micros(16_667)));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct FramePresented {
    /// When the display started showing the frame.
    pub at: EventTime,
    /// The display's refresh interval when it showed the frame, where the platform reports one
    /// that is fixed; `None` for a variable or unknown rate.
    pub refresh: Option<Duration>,
}

impl FramePresented {
    /// A frame shown at `at`, at an unknown refresh rate.
    #[must_use]
    pub const fn new(at: EventTime) -> Self {
        Self { at, refresh: None }
    }

    /// The same report with the display's refresh interval; a zero interval is an unknown one.
    #[must_use]
    pub const fn with_refresh(mut self, refresh: Duration) -> Self {
        self.refresh = if refresh.is_zero() {
            None
        } else {
            Some(refresh)
        };
        self
    }
}
