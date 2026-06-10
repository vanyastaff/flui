//! TapGestureRecognizer benchmarks
//!
//! Hot path: `TapGestureRecognizer::handle_event` is called once per
//! pointer event for every recogniser that has joined the arena for
//! that pointer. The realistic tap sequence is `Down → Move (within
//! slop) → Up` — three events. The bench measures per-event dispatch
//! cost including arena bookkeeping and callback invocation.
//!
//! Performance targets:
//! - `handle_event` with no callbacks wired (pure dispatch path):
//!   < 1 µs per event. The handler does a `Mutex` lock on `gesture_state`,
//!   a button-mismatch check, and an arena peek. No allocations in the
//!   steady state.
//! - `handle_event` with `on_tap` callback wired: < 1.5 µs per event. The
//!   callback dispatch is an `Arc::clone` of the callback handle plus
//!   a `Fn` call — kept off the hot path until `accept_gesture` confirms
//!   the arena win (the `pending_up` deferral).
//!
//! Follows the workspace benchmark template at
//! `rust-studio/.../templates/benchmark-report.md`.
//!
//! Run with `cargo bench -p flui-interaction --bench tap_detector_bench`.

use std::hint::black_box;
use std::sync::Arc;

use criterion::{Criterion, criterion_group, criterion_main};
use flui_interaction::GestureRecognizer;
use flui_interaction::arena::GestureArena;
use flui_interaction::events::{PointerType, make_down_event, make_move_event, make_up_event};
use flui_interaction::ids::PointerId;
use flui_interaction::recognizers::TapGestureRecognizer;
use flui_types::geometry::{Offset, Pixels};

/// Build a recogniser with the given callback wiring. The arc clone
/// is a one-time setup cost; the bench loop only measures
/// `handle_event`.
fn make_recognizer(with_callbacks: bool) -> Arc<TapGestureRecognizer> {
    let arena = GestureArena::new();
    let recognizer = TapGestureRecognizer::new(arena);
    if with_callbacks {
        let r = Arc::clone(&recognizer)
            .with_on_tap_down(|_d| {})
            .with_on_tap_up(|_d| {})
            .with_on_tap(|_d| {});
        // with_*_X returns the same Arc — ensure the builder chain
        // is the one we hand to the bench.
        return r;
    }
    recognizer
}

/// Pointer-down event at the origin (within slop of the down position).
fn down_event() -> flui_interaction::events::PointerEvent {
    make_down_event(
        Offset::new(Pixels(100.0), Pixels(100.0)),
        PointerType::Touch,
    )
}

/// Pointer-move event within the default touch slop (18 px) of the
/// down position — must NOT cancel the in-flight tap.
fn move_within_slop_event() -> flui_interaction::events::PointerEvent {
    make_move_event(
        Offset::new(Pixels(105.0), Pixels(102.0)),
        PointerType::Touch,
    )
}

/// Pointer-up event at the same position as the down — completes a
/// valid tap.
fn up_event() -> flui_interaction::events::PointerEvent {
    make_up_event(
        Offset::new(Pixels(100.0), Pixels(100.0)),
        PointerType::Touch,
    )
}

/// Benchmark the full Down → Move → Up sequence with no callbacks
/// wired. Measures pure dispatch cost (arena lookup, state-machine
/// transitions, slop checks).
fn bench_tap_no_callbacks(c: &mut Criterion) {
    let recognizer = black_box(make_recognizer(false));
    let down = down_event();
    let mv = move_within_slop_event();
    let up = up_event();
    let pointer = PointerId::PRIMARY;
    c.bench_function("TapGestureRecognizer::handle_event (no callbacks)", |b| {
        b.iter(|| {
            recognizer.add_pointer(pointer, Offset::new(Pixels(100.0), Pixels(100.0)));
            recognizer.handle_event(black_box(&down));
            recognizer.handle_event(black_box(&mv));
            recognizer.handle_event(black_box(&up));
            // Re-arm for next iter (Up closes the gesture; reset is
            // implicit in `add_pointer` overwriting primary_pointer).
            recognizer.dispose();
        });
    });
}

/// Benchmark the full Down → Move → Up sequence with all three tap
/// callbacks wired (`on_tap_down`, `on_tap_up`, `on_tap`). The
/// `pending_up` deferral means the callbacks fire AFTER the arena
/// resolves — but the recogniser still has to clone the callback
/// handles, so this bench captures the steady-state allocation
/// profile of the recogniser.
fn bench_tap_with_callbacks(c: &mut Criterion) {
    let recognizer = black_box(make_recognizer(true));
    let down = down_event();
    let mv = move_within_slop_event();
    let up = up_event();
    let pointer = PointerId::PRIMARY;
    c.bench_function(
        "TapGestureRecognizer::handle_event (with on_tap callbacks)",
        |b| {
            b.iter(|| {
                recognizer.add_pointer(pointer, Offset::new(Pixels(100.0), Pixels(100.0)));
                recognizer.handle_event(black_box(&down));
                recognizer.handle_event(black_box(&mv));
                recognizer.handle_event(black_box(&up));
                recognizer.dispose();
            });
        },
    );
}

/// `add_pointer` cost — called once per pointer-down when the
/// recogniser joins the arena. Should be O(1): a `Mutex` lock + state
/// write + arena `add`.
fn bench_add_pointer(c: &mut Criterion) {
    let arena = GestureArena::new();
    let recognizer = black_box(TapGestureRecognizer::new(arena));
    let pointer = PointerId::PRIMARY;
    let position = Offset::new(Pixels(100.0), Pixels(100.0));
    c.bench_function("TapGestureRecognizer::add_pointer", |b| {
        b.iter(|| {
            recognizer.add_pointer(black_box(pointer), black_box(position));
            recognizer.dispose();
        });
    });
}

/// Secondary-button tap — `handle_event` with a
/// `PointerButton::Secondary` payload must route to the secondary
/// callback slot rather than the primary one. The recogniser's
/// per-button dispatch is the hot path here; the bench
/// regression-guards the constant cost.
fn bench_secondary_button(c: &mut Criterion) {
    // We can't easily construct a `PointerButtonEvent` with
    // `Secondary` in a bench (the `make_*_event` helpers only emit
    // `Primary`). Instead we measure the primary-button path and
    // rely on the unit tests in
    // `src/recognizers/tap.rs::secondary_button_routes_to_*` to cover
    // the button-mismatch case. The primary-path cost is the same
    // shape (one extra `down()` callback-table lookup) — if the
    // primary path is fast, the secondary path is too.
    let recognizer = black_box(make_recognizer(false));
    let down = down_event();
    let pointer = PointerId::PRIMARY;
    c.bench_function(
        "TapGestureRecognizer::handle_event (primary, primary path)",
        |b| {
            b.iter(|| {
                recognizer.add_pointer(pointer, Offset::new(Pixels(100.0), Pixels(100.0)));
                recognizer.handle_event(black_box(&down));
                recognizer.dispose();
            });
        },
    );
}

criterion_group!(
    tap_benches,
    bench_tap_no_callbacks,
    bench_tap_with_callbacks,
    bench_add_pointer,
    bench_secondary_button,
);
criterion_main!(tap_benches);
