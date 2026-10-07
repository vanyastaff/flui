//! Static tap dispatch and admission, with a fresh live contact per iteration.
//!
//! Fixture creation and disposal are outside Criterion's measured interval.
//! Each sequence is also run once before measurement to check arena settlement
//! and callback delivery. A permanently disposed recognizer cannot stand in
//! for the dispatch hot path.

use std::{cell::Cell, hint::black_box, rc::Rc};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use flui_foundation::geometry::Offset;
use flui_interaction::arena::GestureArena;
use flui_interaction::events::{
    PointerButton, PointerEvent, PointerType, make_down_event_for_id_with_button,
    make_move_event_for_id, make_up_event_for_id_with_button,
};
use flui_interaction::{GestureRecognizer, PointerDispatch, PointerId, TapGestureRecognizer};

struct TapFixture {
    recognizer: Rc<TapGestureRecognizer>,
    arena: GestureArena,
    callbacks: Rc<Cell<u32>>,
}

impl TapFixture {
    fn new(callbacks: bool, button: PointerButton) -> Self {
        let arena = GestureArena::new();
        let mut recognizer = TapGestureRecognizer::new(arena.clone());
        let calls = Rc::new(Cell::new(0));
        if callbacks {
            let (down, up, tap) = (Rc::clone(&calls), Rc::clone(&calls), Rc::clone(&calls));
            recognizer = match button {
                PointerButton::Secondary => recognizer
                    .with_on_secondary_tap_down(move |_| down.set(down.get() + 1))
                    .with_on_secondary_tap_up(move |_| up.set(up.get() + 1))
                    .with_on_secondary_tap(move |_| tap.set(tap.get() + 1)),
                _ => recognizer
                    .with_on_tap_down(move |_| down.set(down.get() + 1))
                    .with_on_tap_up(move |_| up.set(up.get() + 1))
                    .with_on_tap(move |_| tap.set(tap.get() + 1)),
            };
        }
        Self {
            recognizer,
            arena,
            callbacks: calls,
        }
    }

    fn sequence(&self, events: &[PointerEvent; 3]) {
        self.recognizer
            .add_pointer_down(PointerDispatch::at_root(black_box(&events[0])));
        for event in &events[1..] {
            self.recognizer
                .handle_event(PointerDispatch::at_root(black_box(event)));
        }
        black_box(self.callbacks.get());
    }
}

impl Drop for TapFixture {
    fn drop(&mut self) {
        self.recognizer.dispose();
    }
}

fn events(button: PointerButton) -> [PointerEvent; 3] {
    let pointer = PointerId::PRIMARY;
    let position = Offset::new(100.0, 100.0);
    [
        make_down_event_for_id_with_button(pointer, position, PointerType::Touch, button),
        make_move_event_for_id(pointer, Offset::new(101.0, 101.0), PointerType::Touch),
        make_up_event_for_id_with_button(pointer, position, PointerType::Touch, button),
    ]
}

fn bench_tap_sequences(c: &mut Criterion) {
    for (name, callbacks, button) in [
        (
            "handle_event/static/no_callbacks",
            false,
            PointerButton::Primary,
        ),
        (
            "handle_event/static/primary_callbacks",
            true,
            PointerButton::Primary,
        ),
        (
            "handle_event/static/secondary_callbacks",
            true,
            PointerButton::Secondary,
        ),
    ] {
        let events = events(button);
        let witness = TapFixture::new(callbacks, button);
        witness.sequence(&events);
        assert!(
            witness.arena.is_empty(),
            "the measured tap settles its arena"
        );
        assert_eq!(witness.callbacks.get(), if callbacks { 3 } else { 0 });
        c.bench_function(name, |b| {
            b.iter_batched_ref(
                || TapFixture::new(callbacks, button),
                |fixture| fixture.sequence(&events),
                BatchSize::SmallInput,
            );
        });
    }
}

fn bench_admission(c: &mut Criterion) {
    let [down, _, _] = events(PointerButton::Primary);
    c.bench_function("add_pointer/static", |b| {
        b.iter_batched_ref(
            || TapFixture::new(false, PointerButton::Primary),
            |fixture| {
                fixture
                    .recognizer
                    .add_pointer_down(PointerDispatch::at_root(black_box(&down)))
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(tap_benches, bench_tap_sequences, bench_admission);
criterion_main!(tap_benches);
