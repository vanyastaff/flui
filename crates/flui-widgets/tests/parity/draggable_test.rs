//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/draggable_test.dart`
//! (tag `3.44.0`, 63 `testWidgets` cases).
//!
//! ### Why this file is split into two halves
//!
//! `crates/flui-widgets/src/interaction/draggable.rs` and `.../drag_target.rs`
//! document, in depth, the one architectural gap that shapes every case
//! below: FLUI's pointer dispatch (both the production path and this crate's
//! own test harness) resolves the hit-test path **once, at `PointerDown`**,
//! and replays that cached route for every later `Move`/`Up` — there is no
//! capability reachable from widget or gesture-callback code to run a fresh
//! hit test at an arbitrary *later* global position, which is exactly what
//! the oracle's `_DragAvatar.updateDrag` (`WidgetsBinding.hitTestInView`)
//! needs to discover a `DragTarget` the drag has moved onto. Building that
//! reachability is a legitimate, separate-scope change (the same shape of
//! gap as the missing `Overlay.of(context)` ancestor lookup the feedback
//! widget would also need) — not invented silently as a byproduct of this
//! task.
//!
//! Consequently this file proves two *independently real* things rather than
//! one *simulated* end-to-end thing:
//!
//! 1. **`Draggable`'s gesture lifecycle** — genuine pointer dispatch through
//!    `LaidOutScoped`, exercising the real `MultiDragGestureRecognizer`:
//!    start/update/end/cancel, the `child`/`child_when_dragging` swap,
//!    `max_simultaneous_drags`, and axis restriction. Because no target is
//!    ever discovered, every drop is honestly unaccepted here — proven, not
//!    assumed (`drag_end_reports_not_accepted_and_never_fires_completed`).
//! 2. **`DragTargetState`'s accept/candidate/reject/leave protocol** — driven
//!    directly through its production methods (`did_enter`/`did_move`/
//!    `did_leave`/`did_drop`), the same methods a live discovery mechanism
//!    would call once it exists. This is the load-bearing, testable core the
//!    task brief names explicitly.
//!
//! ### Denominator: 63 oracle cases
//!
//! - **12 ported** below (7 `Draggable`-gesture + 5 `DragTargetState`-protocol
//!   listed in each section's own comment).
//! - **Out of scope, with reasons (51 cases, not silently dropped):**
//!   - **Every case needing the feedback overlay to be visually present or
//!     positioned** (`'Feedback has default ...'`, `'... following pointer'`,
//!     `'DragTarget can be dragged'`, all `'feedbackOffset'`/`dragAnchorStrategy`
//!     cases, `rootOverlay`, `ignoringFeedback*`) — no `Overlay.of(context)`
//!     equivalent exists yet (see `draggable.rs` divergence #1). ~15 cases.
//!   - **Every case needing live hit-test-based target discovery**
//!     end-to-end through a real `Draggable` + `DragTarget` pair moving across
//!     each other on screen (`'Drag and drop'`, `'Drag and drop with tap'`,
//!     `'onLeave and onAccept...'`, `'multi drag...'` positional cases,
//!     `'Drag start delay...'`) — this is exactly the gap point 1 above names;
//!     the *protocol itself* is ported (group 2), the *live wiring* is not.
//!     ~20 cases.
//!   - **`LongPressDraggable`** (every `'long press...'` case) —
//!     `DelayedMultiDragGestureRecognizer` does not exist in `flui-interaction`
//!     yet (see `draggable.rs` divergence #3). ~8 cases.
//!   - **Velocity/fling-magnitude assertions on `onDraggableCanceled`** — the
//!     same real-clock non-determinism `dismissible_test.rs`'s module doc
//!     documents for `DragGestureRecognizer::handle_move`; a scripted move
//!     sequence cannot assert a *specific* velocity. ~3 cases.
//!   - **`axis`-affinity / scroll-interaction cases** (`Axis` combined with a
//!     `Scrollable` ancestor) — no scroll-arena-competition scenario is set up
//!     in this file; covered in spirit by `axis_horizontal_zeroes_reported_vertical_delta`
//!     proving the restriction math alone. ~5 cases.

use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use flui_interaction::PointerId;
use flui_rendering::constraints::BoxConstraints;
use flui_types::layout::Axis;
use flui_types::{Color, Offset, geometry::px};
use flui_view::{StatefulView, ViewExt};
use flui_widgets::{ColoredBox, DragTarget, Draggable, DraggableDetails, ErasedDragData, SizedBox};

use crate::common::{LaidOutScoped, lay_out_with_arena, tight};

fn extent() -> BoxConstraints {
    tight(100.0, 100.0)
}

/// `child`'s render type is `RenderDecoratedBox` — distinct from
/// `child_when_dragging`'s `RenderConstrainedBox`, so the swap test can tell which
/// one is currently mounted by render-type presence alone.
fn child() -> ColoredBox {
    ColoredBox::new(Color::rgb(10, 20, 30))
}

/// Advances one nominal frame so a `RebuildHandle::schedule()` call from a
/// gesture callback (drag start/end swapping `child`/`child_when_dragging`)
/// is observed before the next assertion — same idiom as
/// `dismissible_test.rs`'s `settle_one_frame`.
fn settle_one_frame(scoped: &mut LaidOutScoped) {
    scoped.pump(Duration::from_millis(1));
}

fn pointer(n: u64) -> PointerId {
    PointerId::new(n).expect("contact ids start at 1")
}

fn origin() -> Offset {
    Offset::new(px(0.0), px(0.0))
}

/// Type-erases `value` the way a live `Draggable` session would hand a drag's
/// data to a discovered `DragTarget`.
fn erase<T: Send + Sync + 'static>(value: T) -> ErasedDragData {
    Arc::new(value)
}

// ============================================================================
// Group 1 — `Draggable`'s gesture lifecycle (real pointer dispatch)
// ============================================================================
//
// 7 cases: drag started fires once past slop; update reports the real delta;
// axis restriction zeroes the cross component; end reports unaccepted and
// fires canceled, never completed; a platform pointer-cancel also fires
// `on_drag_end` (Flutter's `finishDrag` is unconditional — this project
// found and fixed a divergence from that while building this port, see
// `pointer_cancel_fires_drag_end_before_canceled_with_zero_velocity` below);
// `max_simultaneous_drags(0)` disables dragging entirely; `child_when_dragging`
// swaps in while active and reverts after the drag ends.

#[test]
fn drag_started_fires_once_past_slop() {
    let started = Arc::new(AtomicUsize::new(0));
    let started_for_cb = Arc::clone(&started);
    let widget = Draggable::<i32>::new(child())
        .data(1)
        .on_drag_started(move || {
            started_for_cb.fetch_add(1, Ordering::SeqCst);
        });
    let scoped = lay_out_with_arena(widget, extent());

    scoped.dispatch_pointer_down(50.0, 50.0);
    assert_eq!(
        started.load(Ordering::SeqCst),
        0,
        "a down alone must not start a drag"
    );

    scoped.dispatch_pointer_move(75.0, 50.0); // 25px > 18px touch slop
    assert_eq!(
        started.load(Ordering::SeqCst),
        1,
        "crossing the slop starts exactly one drag"
    );

    scoped.dispatch_pointer_move(90.0, 50.0);
    assert_eq!(
        started.load(Ordering::SeqCst),
        1,
        "further moves within the same contact must not restart the drag"
    );

    scoped.dispatch_pointer_up(90.0, 50.0);
}

#[test]
fn drag_update_reports_delta_after_start() {
    let last_delta = Arc::new(StdMutex::new(None));
    let last_delta_for_cb = Arc::clone(&last_delta);
    let widget = Draggable::<i32>::new(child()).on_drag_update(move |details| {
        *last_delta_for_cb.lock().expect("not poisoned") = Some(details.delta);
    });
    let scoped = lay_out_with_arena(widget, extent());

    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_move(75.0, 50.0); // slop-crossing move: starts the drag
    scoped.dispatch_pointer_move(95.0, 50.0); // the real update: +20px horizontal

    let delta = last_delta
        .lock()
        .expect("not poisoned")
        .expect("on_drag_update fired after the slop-crossing move");
    assert!(
        (delta.dx.0 - 20.0).abs() < 0.01,
        "expected a +20px horizontal delta, got {delta:?}"
    );
    scoped.dispatch_pointer_up(95.0, 50.0);
}

#[test]
fn axis_horizontal_zeroes_reported_vertical_delta() {
    let last_delta = Arc::new(StdMutex::new(None));
    let last_delta_for_cb = Arc::clone(&last_delta);
    let widget = Draggable::<i32>::new(child())
        .axis(Axis::Horizontal)
        .on_drag_update(move |details| {
            *last_delta_for_cb.lock().expect("not poisoned") = Some(details.delta);
        });
    let scoped = lay_out_with_arena(widget, extent());

    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_move(75.0, 50.0); // starts the drag
    scoped.dispatch_pointer_move(75.0, 90.0); // 0px horizontal, +40px vertical

    let delta = last_delta
        .lock()
        .expect("not poisoned")
        .expect("on_drag_update fired");
    assert_eq!(
        delta.dy.0, 0.0,
        "Axis::Horizontal must zero the reported vertical component (_DragAvatar._restrictAxis)"
    );
    scoped.dispatch_pointer_up(75.0, 90.0);
}

#[test]
fn drag_end_reports_not_accepted_and_never_fires_completed() {
    let end_details: Arc<StdMutex<Option<DraggableDetails>>> = Arc::new(StdMutex::new(None));
    let end_for_cb = Arc::clone(&end_details);
    let canceled = Arc::new(AtomicUsize::new(0));
    let canceled_for_cb = Arc::clone(&canceled);
    let completed = Arc::new(AtomicUsize::new(0));
    let completed_for_cb = Arc::clone(&completed);
    let widget = Draggable::<i32>::new(child())
        .on_drag_end(move |details| {
            *end_for_cb.lock().expect("not poisoned") = Some(details);
        })
        .on_draggable_canceled(move |_velocity, _offset| {
            canceled_for_cb.fetch_add(1, Ordering::SeqCst);
        })
        .on_drag_completed(move || {
            completed_for_cb.fetch_add(1, Ordering::SeqCst);
        });
    let mut scoped = lay_out_with_arena(widget, extent());

    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_move(75.0, 50.0);
    scoped.dispatch_pointer_up(75.0, 50.0);
    settle_one_frame(&mut scoped);

    let details = end_details
        .lock()
        .expect("not poisoned")
        .expect("on_drag_end fired");
    assert!(
        !details.was_accepted,
        "no live target discovery exists yet (see draggable.rs's module docs): every drop is unaccepted"
    );
    assert_eq!(
        canceled.load(Ordering::SeqCst),
        1,
        "on_draggable_canceled fires for every unaccepted drop"
    );
    assert_eq!(
        completed.load(Ordering::SeqCst),
        0,
        "on_drag_completed never fires without live target discovery"
    );
}

#[test]
fn pointer_cancel_fires_drag_end_before_canceled_with_zero_velocity() {
    // Flutter's `_DragAvatar.cancel` routes through `finishDrag`, which fires
    // `onDragEnd` unconditionally (zero velocity, unaccepted) before
    // `onDraggableCanceled` — a platform cancel is not a cancel-only path.
    // Building this port's `DragSession::cancel` initially skipped the
    // `on_drag_end` fire; this case is the red-check that caught it.
    let end_count = Arc::new(AtomicUsize::new(0));
    let end_for_cb = Arc::clone(&end_count);
    let canceled_count = Arc::new(AtomicUsize::new(0));
    let canceled_for_cb = Arc::clone(&canceled_count);
    let widget = Draggable::<i32>::new(child())
        .on_drag_end(move |details| {
            assert_eq!(details.velocity.pixels_per_second, origin());
            assert!(!details.was_accepted);
            end_for_cb.fetch_add(1, Ordering::SeqCst);
        })
        .on_draggable_canceled(move |velocity, _offset| {
            assert_eq!(velocity.pixels_per_second, origin());
            canceled_for_cb.fetch_add(1, Ordering::SeqCst);
        });
    let scoped = lay_out_with_arena(widget, extent());

    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_move(75.0, 50.0);
    scoped.dispatch_pointer_cancel(75.0, 50.0);

    assert_eq!(
        end_count.load(Ordering::SeqCst),
        1,
        "on_drag_end must fire for a platform-cancelled drag too"
    );
    assert_eq!(canceled_count.load(Ordering::SeqCst), 1);
}

#[test]
fn max_simultaneous_drags_zero_disables_dragging() {
    let started = Arc::new(AtomicUsize::new(0));
    let started_for_cb = Arc::clone(&started);
    let widget = Draggable::<i32>::new(child())
        .max_simultaneous_drags(0)
        .on_drag_started(move || {
            started_for_cb.fetch_add(1, Ordering::SeqCst);
        });
    let scoped = lay_out_with_arena(widget, extent());

    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_move(80.0, 50.0);
    scoped.dispatch_pointer_up(80.0, 50.0);

    assert_eq!(
        started.load(Ordering::SeqCst),
        0,
        "max_simultaneous_drags(0) must prevent any drag from ever starting"
    );
}

#[test]
fn child_when_dragging_swaps_in_while_active_and_reverts_on_end() {
    // `child_when_dragging` is a `SizedBox` wrapping its own `ColoredBox`,
    // filling the same 100x100 extent as `child` (matching `extent()`) —
    // not a bare `SizedBox`. `Listener`'s default `HitTestBehavior::DeferToChild`
    // means it only registers a hit where its *current* child does, and
    // `RenderConstrainedBox::hit_test` (`constrained_box.rs`) returns `false`
    // outright when it has no child of its own — a bare `SizedBox` would
    // silently stop the in-flight drag's own `Listener` from ever receiving
    // its `up` once the swap took effect. This case's own red-check caught
    // exactly that while building this port: `on_drag_end`/`end()` stopped
    // firing the moment `child_when_dragging` had no hit-testable content.
    let widget = Draggable::<i32>::new(child()).child_when_dragging(|| {
        SizedBox::new(100.0, 100.0)
            .child(ColoredBox::new(Color::rgb(90, 90, 90)))
            .boxed()
    });
    let mut scoped = lay_out_with_arena(widget, extent());

    assert_eq!(
        scoped
            .laid()
            .find_all_by_render_type("RenderConstrainedBox")
            .len(),
        0,
        "at rest, `child_when_dragging`'s wrapper must not be mounted"
    );

    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_move(80.0, 50.0);
    settle_one_frame(&mut scoped);

    assert_eq!(
        scoped
            .laid()
            .find_all_by_render_type("RenderConstrainedBox")
            .len(),
        1,
        "mid-drag, `child_when_dragging` (a SizedBox) is mounted"
    );

    scoped.dispatch_pointer_up(80.0, 50.0);
    settle_one_frame(&mut scoped);

    assert_eq!(
        scoped
            .laid()
            .find_all_by_render_type("RenderConstrainedBox")
            .len(),
        0,
        "after the drag ends, `child_when_dragging` is unmounted again and `child` is back"
    );
    assert_eq!(
        scoped
            .laid()
            .find_all_by_render_type("RenderDecoratedBox")
            .len(),
        1,
        "after the drag ends, `child` (the original ColoredBox) is remounted"
    );
}

// ============================================================================
// Group 2 — `DragTargetState`'s accept/candidate/reject/leave protocol
// ============================================================================
//
// 5 cases, driven directly against the state machine (see the module doc
// above for why): data delivered on an accepted drop; the candidate list
// gains/loses entries across enter/leave; `on_will_accept` returning `false`
// routes to the rejected count instead of the candidate list; typed-data
// mismatch (a `DragTarget<String>` given an `i32` payload) is always
// rejected regardless of `on_will_accept`; `did_drop` only accepts a pointer
// that is a *current* candidate (a rejected or unknown pointer's drop is a
// no-op, matching the oracle's `assert(_candidateAvatars.contains(avatar))`).

fn string_target() -> DragTarget<String> {
    DragTarget::new(|_candidates, _rejected| SizedBox::new(0.0, 0.0).boxed())
}

#[test]
fn data_delivered_to_on_accept_on_drop() {
    let accepted = Arc::new(StdMutex::new(None));
    let accepted_for_cb = Arc::clone(&accepted);
    let target = string_target().on_accept(move |details| {
        *accepted_for_cb.lock().expect("not poisoned") = Some(details.data);
    });
    let mut state = target.create_state();

    let p = pointer(1);
    assert!(state.did_enter(&target, p, erase("hello".to_string()), origin()));
    assert!(state.did_drop(&target, p, origin()));

    assert_eq!(
        accepted.lock().expect("not poisoned").as_deref(),
        Some("hello"),
        "on_accept must receive the dropped drag's data"
    );
}

#[test]
fn candidate_list_gains_and_loses_entries_across_enter_and_leave() {
    let left_with = Arc::new(StdMutex::new(None));
    let left_for_cb = Arc::clone(&left_with);
    let target = string_target().on_leave(move |data| {
        *left_for_cb.lock().expect("not poisoned") = Some(data);
    });
    let mut state = target.create_state();
    let p = pointer(1);

    assert!(state.candidate_data().is_empty());
    assert!(state.did_enter(&target, p, erase("a".to_string()), origin()));
    assert_eq!(
        state.candidate_data(),
        vec![Some("a".to_string())],
        "an accepted enter must appear in the candidate list"
    );

    state.did_leave(&target, p);
    assert!(
        state.candidate_data().is_empty(),
        "did_leave must remove the pointer from the candidate list"
    );
    assert_eq!(
        left_with.lock().expect("not poisoned").clone().flatten(),
        Some("a".to_string()),
        "on_leave must receive the candidate's data"
    );
}

#[test]
fn on_will_accept_veto_routes_to_rejected_not_candidate() {
    let target = string_target().on_will_accept(|_details| false);
    let mut state = target.create_state();
    let p = pointer(1);

    let accepted = state.did_enter(&target, p, erase("a".to_string()), origin());

    assert!(
        !accepted,
        "on_will_accept returning false must reject the enter"
    );
    assert!(state.candidate_data().is_empty());
    assert_eq!(state.rejected_count(), 1);
}

#[test]
fn typed_data_mismatch_is_always_rejected_regardless_of_on_will_accept() {
    // `on_will_accept` is configured to accept everything — the mismatch
    // must still reject, matching the oracle's `isExpectedDataType` gate,
    // which runs before `onWillAcceptWithDetails` is even consulted.
    let target = string_target().on_will_accept(|_details| true);
    let mut state = target.create_state();
    let p = pointer(1);

    let accepted = state.did_enter(&target, p, erase(42_i32), origin());

    assert!(
        !accepted,
        "an i32 payload must never be accepted by a DragTarget<String>"
    );
    assert!(state.candidate_data().is_empty());
    assert_eq!(state.rejected_count(), 1);
}

#[test]
fn did_drop_only_accepts_a_current_candidate() {
    let accepted = Arc::new(AtomicUsize::new(0));
    let accepted_for_cb = Arc::clone(&accepted);
    let target = string_target()
        .on_will_accept(|_details| false)
        .on_accept(move |_details| {
            accepted_for_cb.fetch_add(1, Ordering::SeqCst);
        });
    let mut state = target.create_state();
    let p = pointer(1);

    assert!(!state.did_enter(&target, p, erase("a".to_string()), origin()));
    let dropped = state.did_drop(&target, p, origin());

    assert!(!dropped, "a rejected pointer's drop must be a no-op");
    assert_eq!(
        accepted.load(Ordering::SeqCst),
        0,
        "on_accept must not fire for a drop that was never a candidate"
    );

    // An unknown pointer (never entered at all) is equally a no-op.
    assert!(!state.did_drop(&target, pointer(2), origin()));
}
