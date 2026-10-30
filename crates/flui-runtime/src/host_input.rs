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

use flui_platform::{EventTime, FramePresented, PlatformInput};

use crate::owner::WindowObservation;

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
        /// When the platform says the frame will be shown, if it says (ADR-0257): idle work
        /// runs only until then.
        target_present: Option<EventTime>,
        /// The platform's report that an earlier frame reached the screen, on the same timeline
        /// as the input before it: the presentation's frame clock keeps the newest, and a frame
        /// whose request names no `target_present` bounds its idle work by the next refresh
        /// the report places.
        presented: Option<FramePresented>,
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
