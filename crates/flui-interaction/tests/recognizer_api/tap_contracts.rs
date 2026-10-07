use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use flui_foundation::geometry::Offset;
use flui_interaction::{
    CancelOutcome, DoubleTapGestureRecognizer, GestureArena, GestureRecognizer,
    TapGestureRecognizer,
    events::{PointerType, make_down_event, make_up_event},
    routing::PointerDispatch,
    traits::PointerEventExtTrait,
};

#[test]
fn tap_builder_lifecycle_contract() {
    for (name, row) in [
        ("cancel_reuses_tap", cancel_reuses_tap as fn()),
        (
            "panicking_cancel_callback_cannot_strand_tap_tracking",
            panicking_cancel_callback_cannot_strand_tap_tracking,
        ),
        (
            "cancel_during_up_suppresses_tap",
            cancel_during_up_suppresses_tap,
        ),
        ("cancel_reuses_double_tap", cancel_reuses_double_tap),
        (
            "replacing_builder_callback_preserves_retirement_failure",
            replacing_builder_callback_preserves_retirement_failure,
        ),
    ] {
        if let Err(payload) = std::panic::catch_unwind(row) {
            eprintln!("tap contract `{name}` failed");
            std::panic::resume_unwind(payload);
        }
    }
}

struct BuilderCapture {
    dropped: Rc<Cell<u32>>,
    panic_on_drop: bool,
}
impl Drop for BuilderCapture {
    fn drop(&mut self) {
        self.dropped.set(self.dropped.get() + 1);
        assert!(!self.panic_on_drop, "old callback drop");
    }
}

fn replacing_builder_callback_preserves_retirement_failure() {
    let arena = GestureArena::new();
    let old_drops = Rc::new(Cell::new(0));
    let incoming_drops = Rc::new(Cell::new(0));
    let old = BuilderCapture {
        dropped: old_drops.clone(),
        panic_on_drop: true,
    };
    let incoming = BuilderCapture {
        dropped: incoming_drops.clone(),
        panic_on_drop: false,
    };
    let builder = TapGestureRecognizer::builder(arena.clone()).on_tap(move |_| {
        std::hint::black_box(&old);
    });
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        drop(builder.on_tap(move |_| {
            std::hint::black_box(&incoming);
        }));
    }))
    .expect_err("old capture failure must resume");
    assert_eq!(failure.downcast_ref::<&str>(), Some(&"old callback drop"));
    assert_eq!(old_drops.get(), 1);
    assert_eq!(
        incoming_drops.get(),
        0,
        "incoming capture must be retained after failure"
    );

    let healthy_drops = Rc::new(Cell::new(0));
    let healthy = BuilderCapture {
        dropped: healthy_drops.clone(),
        panic_on_drop: false,
    };
    let fresh = TapGestureRecognizer::builder(arena).on_tap(move |_| {
        std::hint::black_box(&healthy);
    });
    drop(fresh);
    assert_eq!(
        healthy_drops.get(),
        1,
        "fresh healthy builder retires normally"
    );
}

fn panicking_cancel_callback_cannot_strand_tap_tracking() {
    let arena = GestureArena::new();
    let cancels = Rc::new(Cell::new(0));
    let taps = Rc::new(Cell::new(0));
    let recognizer = TapGestureRecognizer::builder(arena.clone())
        .on_tap_cancel({
            let cancels = cancels.clone();
            move |_| {
                cancels.set(cancels.get() + 1);
                panic!("tap cancel panic");
            }
        })
        .on_tap({
            let taps = taps.clone();
            move |_| taps.set(taps.get() + 1)
        })
        .build();
    let down = make_down_event(Offset::ZERO, PointerType::Touch);
    let up = make_up_event(Offset::ZERO, PointerType::Touch);
    recognizer.add_pointer(PointerDispatch::at_root(&down));
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| recognizer.cancel()));
    assert!(failure.is_err());
    assert_eq!(recognizer.cancel(), CancelOutcome::Idle);
    recognizer.add_pointer(PointerDispatch::at_root(&down));
    arena.close(down.pointer_id());
    recognizer.handle_event(PointerDispatch::at_root(&up));
    arena.drain_deferred_resolutions();
    assert_eq!(cancels.get(), 1);
    assert_eq!(taps.get(), 1);
}

fn cancel_reuses_tap() {
    let arena = GestureArena::new();
    let taps = Rc::new(Cell::new(0));
    let cancels = Rc::new(Cell::new(0));
    let recognizer = TapGestureRecognizer::builder(arena.clone())
        .on_tap({
            let taps = taps.clone();
            move |_| taps.set(taps.get() + 1)
        })
        .on_tap_cancel({
            let cancels = cancels.clone();
            move |_| cancels.set(cancels.get() + 1)
        })
        .build();
    let down = make_down_event(Offset::ZERO, PointerType::Touch);
    let up = make_up_event(Offset::ZERO, PointerType::Touch);
    recognizer.add_pointer(PointerDispatch::at_root(&down));
    assert_eq!(recognizer.cancel(), CancelOutcome::Cancelled);
    assert_eq!(recognizer.cancel(), CancelOutcome::Idle);
    recognizer.add_pointer(PointerDispatch::at_root(&down));
    arena.close(down.pointer_id());
    recognizer.handle_event(PointerDispatch::at_root(&up));
    arena.drain_deferred_resolutions();
    assert_eq!(taps.get(), 1);
    assert_eq!(cancels.get(), 1);
}

fn cancel_during_up_suppresses_tap() {
    let arena = GestureArena::new();
    let holder = Rc::new(RefCell::new(None::<Rc<TapGestureRecognizer>>));
    let taps = Rc::new(Cell::new(0));
    let recognizer = TapGestureRecognizer::builder(arena.clone())
        .on_tap_up({
            let holder = holder.clone();
            move |_| {
                holder.borrow().as_ref().expect("recognizer owner").cancel();
            }
        })
        .on_tap({
            let taps = taps.clone();
            move |_| taps.set(taps.get() + 1)
        })
        .build();
    *holder.borrow_mut() = Some(recognizer.clone());
    let down = make_down_event(Offset::ZERO, PointerType::Touch);
    let up = make_up_event(Offset::ZERO, PointerType::Touch);
    recognizer.add_pointer(PointerDispatch::at_root(&down));
    arena.close(down.pointer_id());
    arena.drain_deferred_resolutions();
    recognizer.handle_event(PointerDispatch::at_root(&up));
    assert_eq!(taps.get(), 0);
    holder.borrow_mut().take();
}

fn cancel_reuses_double_tap() {
    let arena = GestureArena::new();
    let doubles = Rc::new(Cell::new(0));
    let recognizer = DoubleTapGestureRecognizer::builder(arena.clone())
        .on_double_tap({
            let doubles = doubles.clone();
            move |_| doubles.set(doubles.get() + 1)
        })
        .build();
    let down = make_down_event(Offset::ZERO, PointerType::Touch);
    let up = make_up_event(Offset::ZERO, PointerType::Touch);
    recognizer.add_pointer(PointerDispatch::at_root(&down));
    assert_eq!(recognizer.cancel(), CancelOutcome::Cancelled);
    assert_eq!(recognizer.cancel(), CancelOutcome::Idle);
    for _ in 0..2 {
        recognizer.add_pointer(PointerDispatch::at_root(&down));
        arena.close(down.pointer_id());
        recognizer.handle_event(PointerDispatch::at_root(&up));
        arena.sweep(down.pointer_id());
    }
    assert_eq!(doubles.get(), 1);
    assert_eq!(recognizer.cancel(), CancelOutcome::Idle);
}
