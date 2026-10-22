//! The frames the owner window delivers while `DoDragDrop`'s modal loop
//! stands in for the main loop.

use super::modal_loop::ModalFrames;
use super::*;
use crate::owner::handlers::OpenWindows;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Windows that count the frames a delivered deadline asks of them.
#[derive(Debug, Default)]
struct CountedWindows {
    frames_asked: AtomicUsize,
}

impl OpenWindows for CountedWindows {
    fn is_empty(&self) -> bool {
        false
    }
}

impl ClassWindows for CountedWindows {
    fn redraw_for_deadline(&self) {
        self.frames_asked.fetch_add(1, Ordering::Relaxed);
    }

    fn paint_invalid(&self) {}
}

/// A due deadline is delivered once per frame generation and per hook: a
/// replacement hook answering the very instant its predecessor was
/// delivered at is new work, not a repeat. Red-check: key the modal
/// delivery by the instant and the frame count alone.
#[test]
fn a_replaced_wake_hook_gets_its_modal_frame() {
    let windows = Arc::new(CountedWindows::default());
    let frames_asked = || windows.frames_asked.load(Ordering::Relaxed);

    let due = web_time::Instant::now();
    let hook = || -> Arc<WakeDeadlineHook> { Arc::new(move || Some(due)) };
    let handlers = Rc::new(RefCell::new(PlatformHandlers::new()));
    // No frame callback counts into this generation: every delivery below
    // is in the same one.
    let modal = ModalFrames::new(
        Rc::clone(&handlers),
        Arc::downgrade(&windows) as Weak<dyn ClassWindows>,
        Rc::default(),
    );
    let deliver = || modal.deliver();

    handlers.borrow_mut().wake_deadline = Some(hook());
    let _ = deliver();
    assert_eq!(frames_asked(), 1, "the due deadline");
    let _ = deliver();
    assert_eq!(
        frames_asked(),
        1,
        "the same answer of the same hook, with no frame since"
    );

    handlers.borrow_mut().wake_deadline = Some(hook());
    let _ = deliver();
    assert_eq!(
        frames_asked(),
        2,
        "the replacement's answer at the same instant"
    );
}
