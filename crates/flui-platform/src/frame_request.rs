//! What a host reports when it asks for a frame.

use crate::EventTime;

/// A request for one frame, on the event timeline of [`EventTime`].
///
/// A backend fills what its platform reports and nothing it would have to invent (ADR-0254):
/// [`vsync`](Self::vsync) is the platform's frame timestamp rebased onto the event epoch, or the
/// backend's own reading of that clock when the request was admitted where the platform reports
/// none, and [`target_present`](Self::target_present) is `None` unless the platform names when
/// this frame will be shown. Neither is a prediction.
///
/// # Examples
///
/// ```
/// use flui_platform::{EventTime, FrameRequest};
///
/// let vsync = EventTime::from_nanos(16_000_000);
/// let shown = EventTime::from_nanos(33_000_000);
/// let request = FrameRequest::new(vsync).with_target_present(shown);
/// assert_eq!(request.vsync, vsync);
/// assert_eq!(request.target_present, Some(shown));
/// assert_eq!(FrameRequest::new(vsync).target_present, None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct FrameRequest {
    /// When the frame's vsync happened.
    pub vsync: EventTime,
    /// When the platform says this frame will be shown, if it says.
    pub target_present: Option<EventTime>,
}

impl FrameRequest {
    /// A request whose vsync is `vsync` and whose present time is unknown.
    #[must_use]
    pub const fn new(vsync: EventTime) -> Self {
        Self {
            vsync,
            target_present: None,
        }
    }

    /// The same request with the platform's target present time.
    #[must_use]
    pub const fn with_target_present(mut self, target_present: EventTime) -> Self {
        self.target_present = Some(target_present);
        self
    }
}

impl From<EventTime> for FrameRequest {
    /// A request that knows only its vsync.
    fn from(vsync: EventTime) -> Self {
        Self::new(vsync)
    }
}
