//! What an AppKit window's delivery reports to the wake pump
//! (`macos::wake_pump`): each frame its handler runs, into the count that
//! lets a delivered wake deadline be armed again (`WakeDeadline`), and the end
//! of each run that delivered more than input, after which the pump looks at
//! the deadline once more. Pure, so every host runs its tests.
//!
//! The look after such a run is what follows a delivery up: the frames a due
//! deadline asks for run in display passes AppKit schedules, at no time the
//! pump can predict, and such a frame may leave the deadline due again or arm
//! a new one; a state or surface change (a window covered, which then owes a
//! frame) reaches the application in a window callback. No owner turn of the
//! loop need follow either, so without that look the deadline they leave
//! would be unseen. A run of input alone asks for no look: input that changes
//! anything asks for a frame, and that frame's run looks.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use flui_platform::WindowHandler;
use flui_platform::input_capabilities::InputCapabilities;

use crate::owner::window_delivery::WindowDelivery;

/// An AppKit window's delivery that reports to the wake pump: the only form
/// an AppKit window is built from (`MacOSWindow::try_new` takes it), so no
/// window's frames go uncounted or unfollowed. Made by [`window_delivery`].
pub(crate) struct FollowedDelivery(WindowDelivery);

impl FollowedDelivery {
    /// The delivery, for the window that holds it.
    pub(crate) fn into_inner(self) -> WindowDelivery {
        self.0
    }
}

/// The delivery of an AppKit window opened with `handler`, whose input
/// devices give it `capabilities`: each frame the handler runs raises
/// `frames`, the pump's count, and `look` asks the pump for a look after each
/// run that delivered more than input. `look` runs no user code (no
/// `tracing` event either): it may run while a panic unwinds.
pub(crate) fn window_delivery(
    handler: Rc<dyn WindowHandler>,
    capabilities: InputCapabilities,
    frames: Arc<AtomicU64>,
    look: impl Fn() + 'static,
) -> FollowedDelivery {
    FollowedDelivery(
        WindowDelivery::counting_frames(handler, capabilities, frames)
            .after_each_run_beyond_input(look),
    )
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    use flui_platform::input_capabilities::InputCapabilities;
    use flui_platform::{EventTime, FrameRequest, ImeEvent, PlatformInput};
    use flui_testing::window_handler::RecordingWindowHandler;

    use super::window_delivery;

    /// What reaches the pump from a window's runs: a frame that ran raises
    /// the pump's count, so the deadline it was asked for can be armed again,
    /// and every run that delivered more than input (a frame, a state change)
    /// ends in a look, however late after the delivery it ran. A run of input
    /// alone leaves the count and asks for no look. Red-check: build the
    /// delivery with `WindowDelivery::new`, without the hook, with
    /// `after_each_run`, or with a hook gated on the frame count.
    #[test]
    fn a_window_run_beyond_input_asks_the_pump_for_a_look() {
        let handler = RecordingWindowHandler::new();
        let frames = Arc::new(AtomicU64::new(0));
        let looks = Rc::new(Cell::new(0_u32));
        let counted = Rc::clone(&looks);
        let delivery = window_delivery(
            handler.clone(),
            InputCapabilities::UNAVAILABLE,
            Arc::clone(&frames),
            move || counted.set(counted.get() + 1),
        )
        .into_inner();

        let _ = delivery.dispatch_input(PlatformInput::Ime(ImeEvent::Enabled));
        assert_eq!(looks.get(), 0, "a run of input alone asks for no look");
        delivery.dispatch_state_changed();
        assert_eq!(looks.get(), 1, "a run of a state change alone asks for one");

        handler.react_to_frame(|_| {});
        delivery.dispatch_frame(FrameRequest::new(EventTime::from_nanos(1)));
        assert_eq!(
            frames.load(Ordering::Relaxed),
            1,
            "a frame that ran is counted"
        );
        assert_eq!(looks.get(), 2, "and followed by a look");
    }
}
