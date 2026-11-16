//! The frames the owner window delivers while `DoDragDrop`'s modal loop
//! stands in for the main loop.

use super::modal_loop::ModalFrames;
use super::*;
use crate::owner::OpenWindows;
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

    #[cfg(feature = "single-instance")]
    fn shows_any(&self) -> bool {
        false
    }

    #[cfg(feature = "single-instance")]
    fn activate_forwarded(&self) {}
}

/// A due deadline is delivered once per frame generation: the application's
/// same answer with no frame since asks for no frame, and after a frame it
/// does again.
#[test]
fn a_due_deadline_gets_one_modal_frame_per_generation() {
    use crate::owner::app_delivery::testing::{AppLog, bound};

    let windows = Arc::new(CountedWindows::default());
    let frames_asked = || windows.frames_asked.load(Ordering::Relaxed);
    let app = AppLog::new();
    app.answer_deadline(Some(web_time::Instant::now()));
    let run = bound(app);
    let frames = Rc::new(FrameCount::default());
    let modal = ModalFrames::new(
        Arc::clone(&run.delivery),
        Arc::downgrade(&windows) as std::sync::Weak<dyn ClassWindows>,
        Rc::clone(&frames),
        std::sync::Weak::new(),
    );

    assert_eq!(modal.deliver(), 1, "a due deadline asks again at once");
    assert_eq!(frames_asked(), 1, "the due deadline");
    assert_eq!(
        modal.deliver(),
        crate::owner::modal_pace::MODAL_FRAME_PACE_MS,
        "a delivered instant no frame serviced is not waited for: the timer keeps its pace"
    );
    assert_eq!(frames_asked(), 1, "the same answer, with no frame since");
    frames.counter().fetch_add(1, Ordering::Relaxed);
    let _ = modal.deliver();
    assert_eq!(frames_asked(), 2, "the same answer after a frame");
    modal.start();
    let _ = modal.deliver();
    assert_eq!(frames_asked(), 3, "a modal loop's first delivery");
}

/// While a modal loop stands in for the main loop, a due turn deadline wakes
/// the owner once and asks no window for a frame; the same instant is
/// delivered again only after an owner turn ran, and a signal that no longer
/// accepts work is not asked for.
#[test]
fn a_due_turn_deadline_wakes_the_owner_from_a_modal_loop() {
    use crate::owner::app_delivery::testing::{AppLog, bound};
    use crate::owner::signal::{OwnerSignal, OwnerTurnSlot};

    let windows = Arc::new(CountedWindows::default());
    let app = AppLog::new();
    let due = web_time::Instant::now();
    app.answer_turn_deadline_with(move || Some(due));
    let run = bound(app);
    let wakes = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&wakes);
    let signal = OwnerSignal::new(Arc::new(move || {
        counted.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }));
    let modal = ModalFrames::new(
        Arc::clone(&run.delivery),
        Arc::downgrade(&windows) as std::sync::Weak<dyn ClassWindows>,
        Rc::new(FrameCount::default()),
        Arc::downgrade(&signal),
    );
    signal.start().expect("the owner starts");
    let slot = Rc::new(OwnerTurnSlot::default());
    signal
        .register_in(&slot, Box::new(|| {}))
        .expect("the owner takes a turn callback");
    wakes.store(0, Ordering::SeqCst);

    let _ = modal.deliver();
    assert_eq!(
        wakes.load(Ordering::SeqCst),
        1,
        "the due instant woke the owner"
    );
    assert_eq!(
        windows.frames_asked.load(Ordering::Relaxed),
        0,
        "and asked no window for a frame"
    );
    let _ = modal.deliver();
    assert_eq!(
        wakes.load(Ordering::SeqCst),
        1,
        "the same instant, no turn since: not delivered again"
    );

    // The owner's turn runs and leaves the instant in place: it is owed still.
    let _ = signal.drive_in(&slot);
    assert_eq!(signal.turns(), 1, "the owner's turn ran");
    let before = wakes.load(Ordering::SeqCst);
    let _ = modal.deliver();
    assert_eq!(
        wakes.load(Ordering::SeqCst),
        before + 1,
        "after a turn, the instant left unserved is delivered again"
    );
}
