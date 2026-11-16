//! What a host reports when it asks for a frame, and what it learned about the frames it showed.

use flui_foundation::FrameStamp;

use crate::{EventTime, RefreshPeriod};

/// A request for one frame, on the event timeline of [`EventTime`].
///
/// A backend fills what its platform reports and nothing it would have to invent (ADR-0254):
/// [`vsync`](Self::vsync) is the platform's frame timestamp rebased onto the event epoch, or the
/// backend's own reading of that clock when the request was admitted where the platform reports
/// none, and [`target_present`](Self::target_present) is `None` unless the platform names when
/// this frame will be shown. Neither is a prediction. [`presented`](Self::presented) is the
/// platform's newest report, since the window's last request, of an earlier frame shown, naming
/// that frame (ADR-0347), so a request and the input before it are measured on one timeline.
///
/// # Examples
///
/// ```
/// use flui_foundation::{
///     FrameEpoch, FrameStamp, PresentationAddress, PresentationId, SurfaceGeneration, UiRuntimeId,
/// };
/// use flui_platform::{EventTime, FramePresented, FrameRequest};
///
/// let vsync = EventTime::from_nanos(16_000_000);
/// let shown = EventTime::from_nanos(33_000_000);
/// let request = FrameRequest::new(vsync).target_present(shown);
/// assert_eq!(request.vsync, vsync);
/// assert_eq!(request.target_present, Some(shown));
/// assert_eq!(FrameRequest::new(vsync).target_present, None);
///
/// let address = PresentationAddress {
///     ui_runtime_id: UiRuntimeId::new(1),
///     presentation_id: PresentationId::new(1),
/// };
/// let frame = FrameStamp::new(address, FrameEpoch::ZERO.next(), SurfaceGeneration::ZERO.next());
/// let earlier = FramePresented::new(frame, EventTime::from_nanos(9_000_000));
/// let request = FrameRequest::new(vsync).presented(earlier);
/// assert_eq!(request.presented, Some(earlier));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct FrameRequest {
    /// When the frame's vsync happened.
    pub vsync: EventTime,
    /// When the platform says this frame will be shown, if it says.
    pub target_present: Option<EventTime>,
    /// The platform's newest report, since the window's last request, of an earlier frame shown,
    /// if it made one. A window whose capabilities say it reports no presentation
    /// ([`WindowCapabilities::reports_presentation`](crate::WindowCapabilities::reports_presentation))
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
    pub const fn target_present(mut self, target_present: EventTime) -> Self {
        self.target_present = Some(target_present);
        self
    }

    /// The same request carrying the platform's report of an earlier frame shown.
    #[must_use]
    pub const fn presented(mut self, presented: FramePresented) -> Self {
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
/// [`frame`](Self::frame) names the frame the report describes: the stamp the raster side handed
/// the window's surface target with that frame's present
/// ([`SurfaceTarget::pre_present_notify`](crate::SurfaceTarget::pre_present_notify)), which the
/// host carried to the platform's answer for that present (a DXGI present number, the feedback of
/// a Wayland commit, a Metal drawable's handler). A report is never matched to a frame by when it
/// arrived: with two or three presents in flight, or a present the display dropped, the frame shown
/// is not the newest one presented, so a host that cannot tell which present was shown reports
/// nothing. A present that shows no frame of the window (a scene lent outside its frames) is never
/// reported.
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
/// use flui_foundation::{
///     FrameEpoch, FrameStamp, PresentationAddress, PresentationId, SurfaceGeneration, UiRuntimeId,
/// };
/// use flui_platform::{EventTime, FramePresented, RefreshPeriod};
///
/// let address = PresentationAddress {
///     ui_runtime_id: UiRuntimeId::new(1),
///     presentation_id: PresentationId::new(1),
/// };
/// let frame = FrameStamp::new(address, FrameEpoch::ZERO.next(), SurfaceGeneration::ZERO.next());
/// let tap = EventTime::from_nanos(1_000_000);
/// let period = RefreshPeriod::try_from(Duration::from_micros(16_667)).expect("60 Hz");
/// let shown = FramePresented::new(frame, EventTime::from_nanos(25_000_000)).refresh(period);
/// assert_eq!(shown.frame, frame);
/// assert_eq!(shown.at.saturating_duration_since(tap), Duration::from_millis(24));
/// assert_eq!(shown.refresh, Some(period));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct FramePresented {
    /// The frame shown: the stamp its present carried.
    pub frame: FrameStamp,
    /// When the display started showing the frame.
    pub at: EventTime,
    /// The display's refresh period when it showed the frame, where the platform reports a fixed
    /// one that is a cadence; `None` for a variable or unknown rate.
    pub refresh: Option<RefreshPeriod>,
}

impl FramePresented {
    /// Frame `frame` shown at `at`, at an unknown refresh rate.
    #[must_use]
    pub const fn new(frame: FrameStamp, at: EventTime) -> Self {
        Self {
            frame,
            at,
            refresh: None,
        }
    }

    /// The same report with the display's refresh period.
    #[must_use]
    pub const fn refresh(mut self, refresh: RefreshPeriod) -> Self {
        self.refresh = Some(refresh);
        self
    }
}
