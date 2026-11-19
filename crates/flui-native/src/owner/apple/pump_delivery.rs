//! What an AppKit window's delivery reports to the wake pump
//! (`macos::wake_pump`): each frame its handler runs, into the count that
//! lets a delivered wake deadline be armed again (`WakeDeadline`), and the end
//! of each run, after which the pump looks at the deadline once more. Pure,
//! so every host runs its tests.
//!
//! The look after a run is what follows a delivery up: the frames a due
//! deadline asks for run in display passes AppKit schedules, at no time the
//! pump can predict, and such a frame may leave the deadline due again or arm
//! a new one. No owner turn need follow a display pass, so without that look
//! a frame that ran late would leave its deadline unseen.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use flui_platform::WindowHandler;
use flui_platform::input_capabilities::InputCapabilities;

use crate::owner::window_delivery::WindowDelivery;

/// The delivery of an AppKit window opened with `handler`, whose input
/// devices give it `capabilities`: each frame the handler runs raises
/// `frames`, the pump's count, and `look` asks the pump for a look after each
/// run of the window's FIFO. `look` runs no user code (no `tracing` event
/// either): it may run while a panic unwinds.
pub(crate) fn window_delivery(
    handler: Rc<dyn WindowHandler>,
    capabilities: InputCapabilities,
    frames: Arc<AtomicU64>,
    look: impl Fn() + Send + Sync + 'static,
) -> WindowDelivery {
    WindowDelivery::counting_frames(handler, capabilities, frames).after_each_run(look)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    use flui_platform::input_capabilities::InputCapabilities;
    use flui_platform::{EventTime, FrameRequest};
    use flui_testing::window_handler::RecordingWindowHandler;

    use super::window_delivery;

    /// A frame the handler runs reaches the pump twice: it raises the pump's
    /// count, so the deadline it was asked for can be armed again, and its
    /// run ends in a look, however late after the delivery it ran. A declined
    /// frame looks too, and leaves the count. Red-check: build the delivery
    /// with `WindowDelivery::new`, or without `after_each_run`.
    #[test]
    fn a_window_frame_raises_the_pump_count_and_asks_for_a_look() {
        let handler = RecordingWindowHandler::new();
        let frames = Arc::new(AtomicU64::new(0));
        let looks = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&looks);
        let delivery = window_delivery(
            handler.clone(),
            InputCapabilities::UNAVAILABLE,
            Arc::clone(&frames),
            move || {
                counted.fetch_add(1, Ordering::Relaxed);
            },
        );

        delivery.dispatch_frame(FrameRequest::new(EventTime::from_nanos(1)));
        assert_eq!(
            frames.load(Ordering::Relaxed),
            0,
            "a declined frame ran nothing"
        );
        assert_eq!(
            looks.load(Ordering::Relaxed),
            1,
            "its run still ends in a look"
        );

        handler.react_to_frame(|_| {});
        delivery.dispatch_frame(FrameRequest::new(EventTime::from_nanos(2)));
        assert_eq!(
            frames.load(Ordering::Relaxed),
            1,
            "a frame that ran is counted"
        );
        assert_eq!(looks.load(Ordering::Relaxed), 2, "and followed by a look");
    }
}
