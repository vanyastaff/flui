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

/// A due deadline is delivered once per frame generation: the application's
/// same answer with no frame since asks for no frame, and after a frame it
/// does again.
#[test]
fn a_due_deadline_gets_one_modal_frame_per_generation() {
    use crate::owner::app_delivery::testing::{RecordingApp, bound};

    let windows = Arc::new(CountedWindows::default());
    let frames_asked = || windows.frames_asked.load(Ordering::Relaxed);
    let app = RecordingApp::new();
    app.answer_deadline(Some(web_time::Instant::now()));
    let run = bound(app);
    let frames = Rc::new(FrameCount::default());
    let modal = ModalFrames::new(
        Arc::clone(&run.delivery),
        Arc::downgrade(&windows) as std::sync::Weak<dyn ClassWindows>,
        Rc::clone(&frames),
    );

    let _ = modal.deliver();
    assert_eq!(frames_asked(), 1, "the due deadline");
    let _ = modal.deliver();
    assert_eq!(frames_asked(), 1, "the same answer, with no frame since");
    frames.counter().fetch_add(1, Ordering::Relaxed);
    let _ = modal.deliver();
    assert_eq!(frames_asked(), 2, "the same answer after a frame");
}
