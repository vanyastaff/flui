//! The frames the owner window delivers while `DoDragDrop`'s modal loop
//! stands in for the main loop.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::geometry::Size;

/// A due deadline is delivered once per frame generation and per hook: a
/// replacement hook answering the very instant its predecessor was
/// delivered at is new work, not a repeat. Red-check: key the modal
/// delivery by the instant and the frame count alone.
#[test]
fn a_replaced_wake_hook_gets_its_modal_frame() {
    let platform = WindowsPlatform::new().expect("native platform");
    // Hidden: a delivered deadline asks a hidden window's frame directly.
    let window = platform
        .open_window(
            WindowOptions::default()
                .with_title("flui modal frames")
                .with_size(Size::new(120.0, 80.0)),
        )
        .expect("window");
    let frames_asked = Arc::new(AtomicUsize::new(0));
    let asked = Arc::clone(&frames_asked);
    window.on_request_frame(Box::new(move || {
        asked.fetch_add(1, Ordering::Relaxed);
    }));

    let due = web_time::Instant::now();
    let hook = || -> Arc<WakeDeadlineHook> { Arc::new(move || Some(due)) };
    let handlers = RefCell::new(PlatformHandlers::new());
    let windows = Arc::downgrade(&platform.windows);
    // No frame callback counts into this generation: every delivery below
    // is in the same one.
    let frames = FrameCount::default();
    let delivered = ModalDelivery::default();
    let deliver =
        || WindowsPlatform::deliver_modal_frames(&handlers, &windows, &frames, &delivered);

    handlers.borrow_mut().wake_deadline = Some(hook());
    let _ = deliver();
    assert_eq!(frames_asked.load(Ordering::Relaxed), 1, "the due deadline");
    let _ = deliver();
    assert_eq!(
        frames_asked.load(Ordering::Relaxed),
        1,
        "the same answer of the same hook, with no frame since"
    );

    handlers.borrow_mut().wake_deadline = Some(hook());
    let _ = deliver();
    assert_eq!(
        frames_asked.load(Ordering::Relaxed),
        2,
        "the replacement's answer at the same instant"
    );
    window.close();
}
