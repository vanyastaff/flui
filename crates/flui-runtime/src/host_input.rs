//! The one input a UI runtime accepts from its host (ADR-0207 §1).
//!
//! Platform input, window observations, frames and assistive-technology
//! requests reach a presentation as one ordered stream of [`HostInput`]
//! values, so whatever influences an arena decision — a focus loss, a scale
//! change, a frame, an AT action — is observable, recordable and replayable in
//! the order the runtime saw it. The runtime unwraps the value: the parts the
//! interaction layer owns go to the presentation's gesture binding, an AT
//! request to its semantics owner. No `flui-interaction` signature names this
//! type.

use flui_foundation::{FrameEpoch, FrameStamp, PresentationAddress, SurfaceGeneration};
use flui_platform::{EventTime, FramePresented, PlatformInput, RefreshPeriod};

use crate::owner::WindowObservation;

/// The platform's report that a frame of the presentation a [`HostInput`] is
/// addressed to reached the screen: a [`FramePresented`] whose frame is named
/// within that presentation, by its epoch and the surface generation it was
/// drawn for (ADR-0347). The presentation the input is delivered to supplies
/// the address ([`Self::of_presentation`]), so a recorded report replays into
/// the presentation that replays it.
///
/// ```rust
/// use flui_foundation::{FrameEpoch, SurfaceGeneration};
/// use flui_platform::EventTime;
/// use flui_runtime::ShownFrame;
///
/// let shown = ShownFrame::new(
///     FrameEpoch::ZERO.next(),
///     SurfaceGeneration::ZERO.next(),
///     EventTime::from_nanos(9_000_000),
/// );
/// assert_eq!(shown.refresh, None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ShownFrame {
    /// The frame shown, by its epoch within the presentation.
    pub epoch: FrameEpoch,
    /// The surface generation the frame was drawn for.
    pub surface_generation: SurfaceGeneration,
    /// When the display started showing the frame.
    pub at: EventTime,
    /// The display's refresh period then, where the platform reports a cadence.
    pub refresh: Option<RefreshPeriod>,
}

impl ShownFrame {
    /// Frame `epoch`, drawn for `surface_generation`, shown at `at` at an
    /// unknown refresh rate.
    #[must_use]
    pub const fn new(
        epoch: FrameEpoch,
        surface_generation: SurfaceGeneration,
        at: EventTime,
    ) -> Self {
        Self {
            epoch,
            surface_generation,
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

    /// The report as the platform makes it for the presentation at `address`.
    #[must_use]
    pub fn of_presentation(self, address: PresentationAddress) -> FramePresented {
        let presented = FramePresented::new(
            FrameStamp::new(address, self.epoch, self.surface_generation),
            self.at,
        );
        match self.refresh {
            Some(refresh) => presented.refresh(refresh),
            None => presented,
        }
    }
}

impl From<FramePresented> for ShownFrame {
    /// The report named within the presentation whose frame it names.
    fn from(presented: FramePresented) -> Self {
        Self {
            epoch: presented.frame.epoch,
            surface_generation: presented.frame.surface_generation,
            at: presented.at,
            refresh: presented.refresh,
        }
    }
}

/// One unit of host input addressed to a presentation.
///
/// The set is closed: there is no source trait. `Accessibility` carries
/// accesskit's 0.x [`ActionRequest`](accesskit::ActionRequest), so an
/// accesskit release that changes it changes this boundary and the recording
/// format with it.
///
/// ```rust
/// use flui_platform::EventTime;
/// use flui_runtime::HostInput;
///
/// let frame = HostInput::Frame {
///     vsync: EventTime::from_nanos(16_666_667),
///     target_present: None,
///     presented: None,
/// };
/// assert!(matches!(frame, HostInput::Frame { .. }));
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum HostInput {
    /// Pointer, keyboard, IME, drag-and-drop or capability input.
    Platform(PlatformInput),
    /// A normalized observation of the presentation's window: metrics, focus,
    /// visibility, hover, display moves, safe area, appearance.
    Window(WindowObservation),
    /// A frame, on the same event timeline as input: its vsync paces the
    /// interaction layer's resampling.
    Frame {
        /// When the frame's vsync happened.
        vsync: EventTime,
        /// When the platform says the frame will be shown, if it says (ADR-0347): idle work
        /// runs only until then.
        target_present: Option<EventTime>,
        /// The platform's report that an earlier frame of the presentation reached the screen,
        /// naming that frame, on the same timeline as the input before it: the presentation's
        /// frame clock keeps the newest, and a frame whose request names no `target_present`
        /// bounds its idle work by the next refresh the report places.
        presented: Option<ShownFrame>,
    },
    /// An assistive-technology action request, resolved by the
    /// presentation's semantics owner.
    Accessibility(accesskit::ActionRequest),
    /// Where the assistive technology's cursor moved: the node it reads, or
    /// `None` once it left the window's tree. Reported by an adapter whose
    /// support is [`Reported`](flui_semantics::AssistiveFocusSupport::Reported),
    /// and by the presentation itself as assistive technology detaches.
    AssistiveFocus(Option<accesskit::NodeId>),
}
