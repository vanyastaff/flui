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

use flui_platform::{EventTime, PlatformInput};

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
/// let frame = HostInput::Frame { vsync: EventTime::from_nanos(16_666_667) };
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
    },
    /// An assistive-technology action request, resolved by the
    /// presentation's semantics owner.
    Accessibility(accesskit::ActionRequest),
}
