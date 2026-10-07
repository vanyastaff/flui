//! Static and weakly attached tap dispatch, with a fresh contact per iteration.
//!
//! Fixture creation and cancellation are outside Criterion's measured interval.
//! Each sequence is also run once before measurement to check arena settlement
//! and callback delivery. Each measured sequence uses a fresh recognizer.

use std::{cell::Cell, hint::black_box, rc::Rc};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use flui_foundation::geometry::Offset;
use flui_interaction::arena::GestureArena;
use flui_interaction::events::{
    PointerButton, PointerEvent, PointerKind, make_down_event_for_id_with_button,
    make_move_event_for_id, make_up_event_for_id_with_button,
};
use flui_interaction::{
    GestureRecognizer, PointerDispatch, PointerId, RecognizerSet, TapGestureRecognizer,
};

struct TapFixture {
    recognizer: Rc<TapGestureRecognizer>,
    arena: GestureArena,
    callbacks: Rc<Cell<u32>>,
}

impl TapFixture {
    fn new(callbacks: bool, button: PointerButton) -> Self {
        let arena = GestureArena::new();
        let mut recognizer = TapGestureRecognizer::builder(arena.clone());
        let calls = Rc::new(Cell::new(0));
        if callbacks {
            let (down, up, tap) = (Rc::clone(&calls), Rc::clone(&calls), Rc::clone(&calls));
            recognizer = match button {
                PointerButton::SECONDARY => recognizer
                    .on_secondary_tap_down(move |_| down.set(down.get() + 1))
                    .on_secondary_tap_up(move |_| up.set(up.get() + 1))
                    .on_secondary_tap(move |_| tap.set(tap.get() + 1)),
                _ => recognizer
                    .on_tap_down(move |_| down.set(down.get() + 1))
                    .on_tap_up(move |_| up.set(up.get() + 1))
                    .on_tap(move |_| tap.set(tap.get() + 1)),
            };
        }
        Self {
            recognizer: recognizer.build(),
            arena,
            callbacks: calls,
        }
    }

    fn sequence(&self, events: &[PointerEvent; 3]) {
        self.recognizer
            .add_pointer(PointerDispatch::at_root(black_box(&events[0])));
        for event in &events[1..] {
            self.recognizer
                .handle_event(PointerDispatch::at_root(black_box(event)));
        }
        black_box(self.callbacks.get());
    }
}

impl Drop for TapFixture {
    fn drop(&mut self) {
        self.recognizer.cancel();
    }
}

struct AttachedTapFixture {
    owner: TapFixture,
    recognizers: RecognizerSet,
}

impl AttachedTapFixture {
    fn new(callbacks: bool, button: PointerButton) -> Self {
        let owner = TapFixture::new(callbacks, button);
        let mut recognizers = RecognizerSet::default();
        recognizers.attach(&owner.recognizer);
        Self { owner, recognizers }
    }

    fn sequence(&self, events: &[PointerEvent; 3]) {
        for event in events {
            self.recognizers
                .dispatch(PointerDispatch::at_root(black_box(event)));
        }
        black_box(self.owner.callbacks.get());
    }
}

fn events(button: PointerButton) -> [PointerEvent; 3] {
    let pointer = PointerId::new(std::num::NonZeroU64::MIN);
    let position = Offset::new(100.0, 100.0);
    [
        make_down_event_for_id_with_button(pointer, position, PointerKind::Touch, button)
            .expect("valid fixture sample"),
        make_move_event_for_id(pointer, Offset::new(101.0, 101.0), PointerKind::Touch)
            .expect("valid fixture sample"),
        make_up_event_for_id_with_button(pointer, position, PointerKind::Touch, button)
            .expect("valid fixture sample"),
    ]
}

fn bench_tap_sequences(c: &mut Criterion) {
    for (name, callbacks, button) in [
        (
            "handle_event/static/no_callbacks",
            false,
            PointerButton::PRIMARY,
        ),
        (
            "handle_event/static/primary_callbacks",
            true,
            PointerButton::PRIMARY,
        ),
        (
            "handle_event/static/secondary_callbacks",
            true,
            PointerButton::SECONDARY,
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
    let [down, _, _] = events(PointerButton::PRIMARY);
    c.bench_function("add_pointer/static", |b| {
        b.iter_batched_ref(
            || TapFixture::new(false, PointerButton::PRIMARY),
            |fixture| {
                fixture
                    .recognizer
                    .add_pointer(PointerDispatch::at_root(black_box(&down)))
            },
            BatchSize::SmallInput,
        );
    });
}

/// These rows have no pre-migration baseline: dyn dispatch did not exist.
/// Compare each with its matching static row measured in the same run.
fn bench_attached_tap_sequences(c: &mut Criterion) {
    for (name, callbacks, button) in [
        (
            "handle_event/dyn/no_callbacks",
            false,
            PointerButton::PRIMARY,
        ),
        (
            "handle_event/dyn/primary_callbacks",
            true,
            PointerButton::PRIMARY,
        ),
        (
            "handle_event/dyn/secondary_callbacks",
            true,
            PointerButton::SECONDARY,
        ),
    ] {
        let events = events(button);
        let witness = AttachedTapFixture::new(callbacks, button);
        witness.sequence(&events);
        assert!(
            witness.owner.arena.is_empty(),
            "the attached tap settles its arena"
        );
        assert_eq!(witness.owner.callbacks.get(), if callbacks { 3 } else { 0 });
        c.bench_function(name, |b| {
            b.iter_batched_ref(
                || AttachedTapFixture::new(callbacks, button),
                |fixture| fixture.sequence(&events),
                BatchSize::SmallInput,
            );
        });
    }
}

criterion_group!(
    tap_benches,
    bench_tap_sequences,
    bench_admission,
    bench_attached_tap_sequences
);
criterion_main!(tap_benches);
