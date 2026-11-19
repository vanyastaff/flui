//! What an AppKit window's delivery reports to the wake pump
//! (`macos::wake_pump`): each frame its handler runs, into the count that
//! lets a delivered wake deadline be armed again (`WakeDeadline`), and the end
//! of each run, after which the pump looks at the deadline once more, as
//! UIKit's loop does after each of its windows' runs. Pure, so every host
//! runs its tests.
//!
//! The look after a run is what follows a delivery up: a frame AppKit runs in
//! a display pass may leave the deadline due again or arm a new one, a state
//! change (a window covered, which then owes a frame) reaches the application
//! in a window callback, and input may arm a deadline with no frame asked (a
//! gesture's). No owner turn of the loop need follow any of them, so without
//! that look the deadline they leave would be unseen. A look is one owner-
//! queue post, coalesced with any already queued.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use flui_platform::WindowHandler;
use flui_platform::input::input_capabilities::InputCapabilities;

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
/// run. `look` runs no user code (no `tracing` event either): it may run
/// while a panic unwinds.
pub(crate) fn window_delivery(
    handler: Rc<dyn WindowHandler>,
    capabilities: InputCapabilities,
    frames: Arc<AtomicU64>,
    look: impl Fn() + 'static,
) -> FollowedDelivery {
    FollowedDelivery(
        WindowDelivery::counting_frames(handler, capabilities, frames).after_each_run(look),
    )
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    use flui_platform::input::input_capabilities::InputCapabilities;
    use flui_platform::{DispatchEventResult, EventTime, FrameRequest, ImeEvent, PlatformInput};
    use flui_testing::window_handler::RecordingWindowHandler;

    use super::window_delivery;

    /// What reaches the pump from a window's runs: a frame that ran raises
    /// the pump's count, so the deadline it was asked for can be armed again,
    /// and every run ends in one look, however late after the delivery it
    /// ran: a run of input alone (which may arm a gesture deadline with no
    /// frame asked), a run of input and the state change raised inside it,
    /// and a frame's. Red-check: build the delivery with
    /// `WindowDelivery::new`, without the hook, or with a hook that skips
    /// runs of input alone.
    #[test]
    fn each_window_run_asks_the_pump_for_a_look() {
        let handler = RecordingWindowHandler::new();
        let frames = Arc::new(AtomicU64::new(0));
        let looks = Rc::new(Cell::new(0_u32));
        let counted = Rc::clone(&looks);
        let delivery = Rc::new(
            window_delivery(
                handler.clone(),
                InputCapabilities::UNAVAILABLE,
                Arc::clone(&frames),
                move || counted.set(counted.get() + 1),
            )
            .into_inner(),
        );

        let _ = delivery.dispatch_input(PlatformInput::Ime(ImeEvent::Enabled));
        assert_eq!(looks.get(), 1, "a run of input alone asks for a look");

        let raising = Rc::downgrade(&delivery);
        handler.react_to_input(move |_| {
            if let Some(delivery) = raising.upgrade() {
                delivery.dispatch_state_changed();
            }
            DispatchEventResult::default()
        });
        let _ = delivery.dispatch_input(PlatformInput::Ime(ImeEvent::Enabled));
        assert_eq!(
            looks.get(),
            2,
            "a run of input and a state change asks once"
        );

        handler.react_to_frame(|_| {});
        delivery.dispatch_frame(FrameRequest::new(EventTime::from_nanos(1)));
        assert_eq!(
            frames.load(Ordering::Relaxed),
            1,
            "a frame that ran is counted"
        );
        assert_eq!(looks.get(), 3, "and followed by a look");
    }
}
