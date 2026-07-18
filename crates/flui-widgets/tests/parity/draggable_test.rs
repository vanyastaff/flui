//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/draggable_test.dart`
//! (tag `3.44.0`, **71** `testWidgets` cases — `grep -cE '^\s*testWidgets\('`
//! against the tagged file).
//!
//! ### Why this file is split into two halves
//!
//! `crates/flui-widgets/src/interaction/draggable.rs` and `.../drag_target.rs`
//! document, in depth, the one architectural gap that shapes most of the
//! cases below: FLUI's pointer dispatch (both the production path and this
//! crate's own test harness) resolves the hit-test path **once, at
//! `PointerDown`**, and replays that cached route for every later
//! `Move`/`Up` — there is no capability reachable from widget or
//! gesture-callback code to run a fresh hit test at an arbitrary *later*
//! global position, which is exactly what the oracle's
//! `_DragAvatar.updateDrag` (`WidgetsBinding.hitTestInView`) needs to
//! discover a `DragTarget` the drag has moved onto. Building that
//! reachability is a legitimate, separate-scope change (the same shape of
//! gap as the missing `Overlay.of(context)` ancestor lookup the feedback
//! widget would also need) — not invented silently as a byproduct of this
//! task. Both gaps are tracked as roadmap follow-ups (Cross.H: widget-reachable
//! fresh hit-test capability; Cross.H: `Overlay.of`-style ancestor lookup).
//!
//! Consequently this file proves two *independently real* things rather than
//! one *simulated* end-to-end thing:
//!
//! 1. **`Draggable`'s gesture lifecycle** — genuine pointer dispatch through
//!    `LaidOutScoped`, exercising the real `MultiDragGestureRecognizer`:
//!    start/update/end/cancel, the `child`/`child_when_dragging` swap,
//!    `max_simultaneous_drags`, axis restriction (including the oracle's
//!    "only fires when the restricted position actually moves" gate), and
//!    the divergent-but-documented immediate-cancel-on-unmount behavior.
//!    Because no target is ever discovered, every drop is honestly
//!    unaccepted here — proven, not assumed
//!    (`drag_end_reports_not_accepted_and_never_fires_completed`).
//! 2. **`DragTargetState`'s accept/candidate/reject/leave protocol** — driven
//!    directly through its production methods (`did_enter`/`did_move`/
//!    `did_leave`/`did_drop`), the same methods a live discovery mechanism
//!    would call once it exists. This is the load-bearing, testable core the
//!    task brief names explicitly.
//!
//! ### Denominator: 71 oracle cases
//!
//! **16 tests ported below** (10 `Draggable`-gesture + 6
//! `DragTargetState`-protocol, listed in each section's own comment). Of
//! these, 4 correspond exactly (or as an explicitly-noted partial) to a
//! specific oracle `testWidgets` name:
//! - `'Null axis onDragUpdate called only if draggable moves in any
//!   direction'`, `'Vertical axis onDragUpdate only called if draggable
//!   moves vertical'`, `'Horizontal axis onDragUpdate only called if
//!   draggable moves horizontal'` → the three `*_on_drag_update_only_fires_*`
//!   tests.
//! - `'Drag and drop - maxSimultaneousDrags'` → `max_simultaneous_drags_zero_disables_dragging`
//!   covers only the `maxSimultaneousDrags: 0` half of that oracle case; the
//!   `maxSimultaneousDrags: 2`-with-3-concurrent-pointers half needs two (or
//!   three) *independently addressable* concurrent contacts, and
//!   `LaidOutScoped`'s `dispatch_pointer_*` sugar tracks exactly one
//!   "current contact" at a time (each `dispatch_pointer_down` reassigns it) —
//!   there is no way through this harness's public surface to `moveTo`/`up`
//!   an *earlier* contact once a later one has gone down. Named harness
//!   limitation, not silently dropped.
//! - `'Draggable disposes recognizer'` / `'Drag and drop - remove
//!   draggable'` → `unmounting_mid_drag_cancels_immediately_and_fires_end_and_canceled`
//!   proves the *documented divergence* (`draggable.rs`'s divergence #5)
//!   in their place, not the oracle's own keep-alive behavior.
//!
//! The remaining 12 ported tests exercise the oracle's established
//! start/update/end/cancel/accept/candidate/reject/leave *contract* —
//! spread by the oracle across many live end-to-end `testWidgets` cases —
//! without a single corresponding `testWidgets` name, same as the original
//! cut of this file.
//!
//! **55 cases out of scope, with reasons (not silently dropped from the
//! count):**
//!
//! - **Feedback overlay presence/position (15 cases).** No
//!   `Overlay.of(context)` equivalent exists (`draggable.rs` divergence #1):
//!   `'Null/Horizontal/Vertical axis draggable moves ...'` (×5, check the
//!   *feedback* widget's rendered position), `'Drag feedback with child
//!   anchor positions correctly'`, `'... within a non-global Overlay ...'`,
//!   `'Drag feedback is put on root overlay with [rootOverlay] flag'` (×2,
//!   duplicate oracle name), `'... matches pointer in scaled MaterialApp'`,
//!   `'childDragAnchorStrategy works in scaled MaterialApp'`, `'... matches
//!   pointer in rotated MaterialApp'`, `'unmounting overlay ends drag
//!   gracefully'`, `'feedback respect the MouseRegion cursor configure'`,
//!   `'configurable feedback ignore pointer behavior'`.
//! - **Live hit-test-based target discovery (29 cases).** Needs a real
//!   `DragTarget` hit-tested at the pointer's current, moved-to position —
//!   the exact gap this module doc opens with: `'control test'`, `'onLeave
//!   callback fires correctly'` (×2, with/without generic param — the
//!   *protocol* is ported directly, see `on_leave` coverage in group 2),
//!   `'onMove callback fires correctly'` (×2, ditto — see
//!   `on_move_fires_for_both_candidate_and_rejected_entries`), `'onMove is
//!   not called if moved with null data'` (also needs null-data modeling —
//!   `ErasedDragData` erases a concrete value, not an `Option`, so a
//!   genuinely-null `Draggable::data` has no representation in this cut),
//!   `'dragging over button'`, `'tapping button'`, `'horizontal and vertical
//!   draggables in vertical/horizontal block'` (×2), `'onDraggableCanceled
//!   not/called if dropped on a/non-accepting target'` (×2, plus `'...with
//!   details'`/`'...with correct velocity'` variants, ×2 more),
//!   `'onDragEnd not called if dropped on non-accepting target'` (+`'...with
//!   details'`, ×2), `'DragTarget rebuilds with and without rejected data
//!   ...'`, `'Can drag and drop over a non-accepting target multiple
//!   times'`, `'onDragCompleted not called if dropped on non-accepting
//!   target'` (+`'...with details'`, ×2), `'onDragEnd called if dropped on
//!   accepting target'`, `'DragTarget does not call onDragEnd when remove
//!   from the tree'`, `'onDragCompleted called if dropped on accepting
//!   target'`, `'allow pass through of unaccepted data test'` (+`'...twice
//!   test'`, ×2), `'onAccept is not called if dropped with null data'` (also
//!   null-data), `'Draggable plays nice with onTap'`, `'DragTarget does not
//!   set state when remove from the tree'` (a `setState`-after-dispose class
//!   of bug Rust's ownership model rules out structurally — same reasoning
//!   as `dismissible_test.rs`'s `'Verify that drag-move events do not
//!   assert'` note).
//! - **`LongPressDraggable` (12 cases).** `DelayedMultiDragGestureRecognizer`
//!   does not exist in `flui-interaction` yet (`draggable.rs` divergence
//!   #3): both `'long press draggable, short/long press'`, `'Tap above
//!   long-press draggable works'`, `'long-press draggable calls onDragEnd/
//!   onDragCompleted/onDragStartedCalled ...'` (×3), `'Custom/Default long
//!   press delay for LongPressDraggable'` (×2), `'long-press draggable calls
//!   Haptic Feedback onStart'`, `'... can disable Haptic Feedback'`,
//!   `'configurable feedback ignore pointer behavior - LongPressDraggable'`,
//!   `'LongPressDraggable.dragAnchorStrategy'`.
//! - **Rust's exact-type `downcast` vs. Dart's supertype-compatible `is`
//!   (3 cases).** `'DragTarget<Object> can accept Draggable<int> data'`,
//!   `'DragTarget<int> can accept Draggable<Object> data when runtime type
//!   is int'`, `'... should not accept ... runtime type null'` — Dart's `is
//!   T?` accepts a subtype/matching-runtime-type value through a wider
//!   static type (`Object`); `Any::downcast::<T>()` requires the *exact*
//!   concrete type `T`, with no variance. A `DragTarget<Object>` has no
//!   Rust equivalent that would accept a boxed `i32` the way Dart's does.
//! - **`hitTestBehavior` not configurable on `Draggable`/`DragTarget` (3
//!   cases).** `'configurable DragTarget hit test behavior'` (×2, duplicate
//!   oracle name), `'configurable Draggable hit test behavior'` — named
//!   deferral (`draggable.rs` divergence #4).
//! - **`allowedButtonsFilter` not implemented (1 case).** `'Test
//!   allowedButtonsFilter'` — named deferral (`draggable.rs` divergence #4).
//! - **Deprecated dual-callback assertion, not applicable (1 case).**
//!   `'throws error when both onWillAccept and onWillAcceptWithDetails are
//!   provided'` — `DragTarget` ships only the details-carrying callback under
//!   the plain name (`drag_target.rs` divergence note); there is no
//!   deprecated predecessor to guard against combining.
//! - **Semantics (1 case).** `'Drag and drop can contribute semantics'` —
//!   Phase 3 (deferred) per this crate's `parity/main.rs` module doc, same
//!   as every other file in this directory.
//! - **Custom `dragAnchorStrategy` callback (1 case).** `'when a
//!   dragAnchorStrategy is provided it gets called'` — only the *default*
//!   strategy's offset semantics are implemented (`draggable.rs` divergence
//!   #4); a caller-supplied strategy function is not configurable.

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

/// A larger box for the axis-update-gate tests below, which move the
/// pointer well past 100px in both dimensions to exercise several
/// independent move steps without crowding `extent()`'s edge.
fn large_extent() -> BoxConstraints {
    tight(300.0, 300.0)
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
// 10 cases: drag started fires once past slop; update reports the RAW delta
// (unrestricted — see the axis-gate cases below); the null/vertical/horizontal
// axis "onDragUpdate only fires if the restricted position actually moved"
// gate (the oracle's own three-case group, ported 1:1); end reports
// unaccepted and fires canceled, never completed, with the tracked
// anchor-relative offset; a platform pointer-cancel also fires `on_drag_end`
// with that same offset and zero velocity (Flutter's `finishDrag` is
// unconditional — this project found and fixed a divergence from that while
// building this port, see `pointer_cancel_fires_drag_end_before_canceled_with_zero_velocity`
// below); `max_simultaneous_drags(0)` disables dragging entirely;
// `child_when_dragging` swaps in while active and reverts after the drag
// ends; unmounting mid-drag cancels immediately (the documented divergence
// from the oracle's recognizer keep-alive).

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

/// Records every `on_drag_update` delta and a running fire count.
#[derive(Default)]
struct UpdateLog {
    fires: usize,
    last_delta: Option<Offset<flui_types::geometry::PixelDelta>>,
}

// Every case below starts the drag with a single, clean, sufficiently large
// move (its own magnitude alone crosses the touch slop — the same pattern
// `drag_started_fires_once_past_slop` uses). This matters because
// `MultiDragGestureRecognizer` flushes the *accumulated* pending delta as
// the drag's first `update()` call the moment slop is crossed (see
// `multidrag.rs`'s "pending delta flushes on acceptance" contract); spreading
// slop-crossing across two small moves would fold both of their deltas into
// that first flush, making a single move's contribution unobservable. A
// single large first move sidesteps that entirely: it both starts the drag
// and *is* the move under test, and every move after it is a clean,
// unaccumulated per-move delta.

#[test]
fn null_axis_on_drag_update_only_fires_when_position_moves() {
    // Oracle: `'Null axis onDragUpdate called only if draggable moves in any
    // direction'`. A zero-delta move must not fire a second update.
    let log = Arc::new(StdMutex::new(UpdateLog::default()));
    let log_for_cb = Arc::clone(&log);
    let widget = Draggable::<i32>::new(child()).on_drag_update(move |details| {
        let mut log = log_for_cb.lock().expect("not poisoned");
        log.fires += 1;
        log.last_delta = Some(details.delta);
    });
    let scoped = lay_out_with_arena(widget, large_extent());

    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_move(80.0, 80.0); // +30,+30: starts the drag AND fires (nonzero)
    assert_eq!(
        log.lock().expect("not poisoned").fires,
        1,
        "the slop-crossing move itself carries a nonzero delta and must fire"
    );

    scoped.dispatch_pointer_move(80.0, 80.0); // zero delta from the last position
    assert_eq!(
        log.lock().expect("not poisoned").fires,
        1,
        "a zero-delta move must not fire again"
    );
    scoped.dispatch_pointer_up(80.0, 80.0);
}

#[test]
fn vertical_axis_on_drag_update_only_fires_when_position_moves_vertically() {
    // Oracle: `'Vertical axis onDragUpdate only called if draggable moves
    // vertical'`. A purely-horizontal move must not fire (the restricted
    // position does not change), even though it carries a nonzero raw delta.
    let log = Arc::new(StdMutex::new(UpdateLog::default()));
    let log_for_cb = Arc::clone(&log);
    let widget = Draggable::<i32>::new(child())
        .axis(Axis::Vertical)
        .on_drag_update(move |details| {
            let mut log = log_for_cb.lock().expect("not poisoned");
            log.fires += 1;
            log.last_delta = Some(details.delta);
        });
    let scoped = lay_out_with_arena(widget, large_extent());

    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_move(50.0, 90.0); // +0,+40: starts the drag, purely vertical
    assert_eq!(
        log.lock().expect("not poisoned").fires,
        1,
        "a vertical move must fire under Axis::Vertical"
    );

    scoped.dispatch_pointer_move(90.0, 90.0); // +40,+0 from here: purely horizontal
    assert_eq!(
        log.lock().expect("not poisoned").fires,
        1,
        "a purely horizontal move must not fire under Axis::Vertical, even \
         though its raw delta is nonzero"
    );
    scoped.dispatch_pointer_up(90.0, 90.0);
}

#[test]
fn horizontal_axis_on_drag_update_only_fires_when_position_moves_horizontally() {
    // Oracle: `'Horizontal axis onDragUpdate only called if draggable moves
    // horizontal'` — the mirror of the vertical case above, plus a final
    // diagonal move proving the *reported* delta is RAW (unrestricted), not
    // axis-zeroed, per `draggable.rs`'s divergence from the earlier cut.
    let log = Arc::new(StdMutex::new(UpdateLog::default()));
    let log_for_cb = Arc::clone(&log);
    let widget = Draggable::<i32>::new(child())
        .axis(Axis::Horizontal)
        .on_drag_update(move |details| {
            let mut log = log_for_cb.lock().expect("not poisoned");
            log.fires += 1;
            log.last_delta = Some(details.delta);
        });
    let scoped = lay_out_with_arena(widget, large_extent());

    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_move(90.0, 50.0); // +40,+0: starts the drag, purely horizontal
    assert_eq!(
        log.lock().expect("not poisoned").fires,
        1,
        "a horizontal move must fire under Axis::Horizontal"
    );

    scoped.dispatch_pointer_move(90.0, 90.0); // +0,+40 from here: purely vertical
    assert_eq!(
        log.lock().expect("not poisoned").fires,
        1,
        "a purely vertical move must not fire under Axis::Horizontal"
    );

    scoped.dispatch_pointer_move(110.0, 120.0); // +20,+30: on-axis component present, fires
    assert_eq!(log.lock().expect("not poisoned").fires, 2);
    let delta = log.lock().expect("not poisoned").last_delta.expect("fired");
    assert!(
        (delta.dx.0 - 20.0).abs() < 0.01 && (delta.dy.0 - 30.0).abs() < 0.01,
        "the RAW (unrestricted) delta is reported — both components — not \
         axis-zeroed: got {delta:?}"
    );
    scoped.dispatch_pointer_up(110.0, 120.0);
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
    // `onDragEnd` unconditionally (zero velocity, unaccepted, but the real
    // `_lastOffset`) before `onDraggableCanceled` — a platform cancel is not
    // a cancel-only path. Building this port's `DragSession::cancel`
    // initially skipped the `on_drag_end` fire; this case is the red-check
    // that caught it.
    let end_count = Arc::new(AtomicUsize::new(0));
    let end_for_cb = Arc::clone(&end_count);
    let canceled_count = Arc::new(AtomicUsize::new(0));
    let canceled_for_cb = Arc::clone(&canceled_count);
    let widget = Draggable::<i32>::new(child())
        .on_drag_end(move |details| {
            assert_eq!(details.velocity.pixels_per_second, origin());
            assert!(!details.was_accepted);
            // The tracked anchor-relative offset (25px right, matching the
            // move below), not zero — only velocity is zero on cancel.
            assert!((details.offset.dx.0 - 25.0).abs() < 0.01);
            end_for_cb.fetch_add(1, Ordering::SeqCst);
        })
        .on_draggable_canceled(move |velocity, offset| {
            assert_eq!(velocity.pixels_per_second, origin());
            assert!((offset.dx.0 - 25.0).abs() < 0.01);
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
    // Oracle: `'Drag and drop - maxSimultaneousDrags'` — the
    // `maxSimultaneousDrags: 0` half only; see the module doc on why the
    // N-concurrent-pointers half is a named harness limitation.
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

#[test]
fn unmounting_mid_drag_cancels_immediately_and_fires_end_and_canceled() {
    // Oracle: `'Draggable disposes recognizer'` / `'Drag and drop - remove
    // draggable'`, in spirit — see `draggable.rs`'s divergence #5 on why
    // this port cancels immediately on unmount instead of tracking the
    // pointer to its real up (a `MultiDragGestureRecognizer` is `!Send +
    // !Sync`, so a `Send + Sync`-bound `DragSession` cannot hold a
    // reference to it to dispose it later itself).
    //
    // `Draggable`'s own gesture recognition (crossing the drag slop) needs
    // the arena-driven dispatch `lay_out_with_arena`/`LaidOutScoped`
    // provides (plain `lay_out`'s dispatch never starts the drag at all —
    // confirmed directly while building this case). But swapping the
    // *literal root element's own type* via `pump_widget`/`swap_root_view`
    // does not run the normal unmount/dispose path either (also confirmed
    // directly: a bare-root swap never calls `DraggableState::dispose`, with
    // or without an active drag) — `dismissible_test.rs`'s own unmount cases
    // hit the same thing and avoid it by keeping an outer wrapper (there,
    // `VsyncScope`) stable and swapping only its child. Here, that means
    // re-wrapping the replacement root in a `GestureArenaScope` built from
    // the *same* arena (`LaidOut::arena()`), so `GestureArenaScope` itself —
    // the true root element — never changes type across the swap.
    //
    // The stable wrapper is `Padding::new(EdgeInsets::ZERO)`, not `Center`:
    // `Center` gives its child its own preferred (loose) size, which shrank
    // `Draggable`'s `ColoredBox` away from the coordinates this test dispatches
    // to (confirmed directly — the drag never started under `Center`). Zero
    // padding passes the incoming tight `extent()` constraint straight
    // through, so `child()`'s `ColoredBox` still fills the whole box exactly
    // as it does everywhere else in this file.
    use flui_types::geometry::EdgeInsets;
    use flui_widgets::{GestureArenaScope, Padding};

    let started = Arc::new(AtomicUsize::new(0));
    let started_for_cb = Arc::clone(&started);
    let canceled = Arc::new(AtomicUsize::new(0));
    let canceled_for_cb = Arc::clone(&canceled);
    let widget = Draggable::<i32>::new(child())
        .on_drag_started(move || {
            started_for_cb.fetch_add(1, Ordering::SeqCst);
        })
        .on_draggable_canceled(move |_v, _o| {
            canceled_for_cb.fetch_add(1, Ordering::SeqCst);
        });
    let mut scoped = lay_out_with_arena(Padding::new(EdgeInsets::ZERO).child(widget), extent());

    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_move(80.0, 50.0);
    assert_eq!(
        started.load(Ordering::SeqCst),
        1,
        "the drag must have started before we test unmounting it mid-flight"
    );
    assert_eq!(
        canceled.load(Ordering::SeqCst),
        0,
        "the drag is active; no cancellation yet"
    );

    // Unmount the Draggable (swap Padding's child for an unrelated widget)
    // while the drag is still active. The GestureArenaScope root and the
    // Padding underneath it both stay mounted; only Padding's child changes.
    let arena = scoped.laid().arena();
    scoped.pump_widget(GestureArenaScope::new(
        arena,
        Padding::new(EdgeInsets::ZERO).child(child()),
    ));

    assert_eq!(
        canceled.load(Ordering::SeqCst),
        1,
        "unmounting mid-drag must dispose the recognizer, which cancels the \
         still-active drag right there"
    );
}

// ============================================================================
// Group 2 — `DragTargetState`'s accept/candidate/reject/leave protocol
// ============================================================================
//
// 6 cases, driven directly against the state machine (see the module doc
// above for why): data delivered on an accepted drop; the candidate list
// gains/loses entries across enter/leave; `on_will_accept` returning `false`
// routes to the rejected list instead of the candidate list; typed-data
// mismatch (a `DragTarget<String>` given an `i32` payload) never becomes an
// entry at all, matching the oracle's discovery-time `isExpectedDataType`
// filter; `did_move` fires for a rejected entry too — the oracle's own
// `didMove` gates only on null data, not rejection status (this is the
// mutation-coverage gap the pre-fix code had: `did_move` used to no-op for
// `Standing::Rejected`); `did_drop` only accepts a *current* candidate.

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
    let left_with = Arc::new(StdMutex::new(None));
    let left_for_cb = Arc::clone(&left_with);
    let target = string_target()
        .on_will_accept(|_details| false)
        .on_leave(move |data| {
            *left_for_cb.lock().expect("not poisoned") = Some(data);
        });
    let mut state = target.create_state();
    let p = pointer(1);

    let accepted = state.did_enter(&target, p, erase("a".to_string()), origin());

    assert!(
        !accepted,
        "on_will_accept returning false must reject the enter"
    );
    assert!(state.candidate_data().is_empty());
    assert_eq!(
        state.rejected_data(),
        vec!["a".to_string()],
        "a vetoed-but-correctly-typed drag is tracked with its real data, \
         not merely counted (the oracle's own _rejectedAvatars only ever \
         holds T?-typed data — see drag_target.rs's module docs)"
    );

    state.did_leave(&target, p);
    assert_eq!(
        left_with.lock().expect("not poisoned").clone().flatten(),
        Some("a".to_string()),
        "on_leave must receive the rejected entry's real data too — the \
         oracle's didLeave does `avatar.data as T?` unconditionally, not \
         gated on candidate-vs-rejected status"
    );
}

#[test]
fn typed_data_mismatch_is_never_tracked_regardless_of_on_will_accept() {
    // `on_will_accept` is configured to accept everything — the mismatch
    // must still be rejected, and moreover never becomes an entry at all
    // (neither candidate nor rejected): the oracle's `_getDragTargets`
    // filters by `isExpectedDataType` *before* `didEnter` ever runs, so a
    // type-mismatched avatar never reaches `_candidateAvatars` OR
    // `_rejectedAvatars`.
    let target = string_target().on_will_accept(|_details| true);
    let mut state = target.create_state();
    let p = pointer(1);

    let accepted = state.did_enter(&target, p, erase(42_i32), origin());

    assert!(
        !accepted,
        "an i32 payload must never be accepted by a DragTarget<String>"
    );
    assert!(state.candidate_data().is_empty());
    assert!(
        state.rejected_data().is_empty(),
        "a type mismatch must not be tracked as rejected either — it never \
         became an entry at all"
    );

    // Confirms it was never tracked: a later did_leave/did_move/did_drop for
    // the same pointer is a no-op, not an error.
    state.did_leave(&target, p);
    state.did_move(&target, p, origin());
    assert!(!state.did_drop(&target, p, origin()));
}

#[test]
fn did_move_fires_for_both_candidate_and_rejected_entries() {
    // Oracle: `_DragTargetState.didMove`'s only gate is `avatar.data ==
    // null` — a genuinely null payload — NOT rejection status. A
    // veto-rejected-but-correctly-typed avatar still sits in
    // `_enteredTargets` and receives every move. This is the mutation-
    // coverage gap the original cut of this file had: `did_move` used to
    // silently no-op for `Standing::Rejected`, which no test here caught
    // until this case was added.
    let candidate_moves = Arc::new(StdMutex::new(Vec::new()));
    let candidate_moves_for_cb = Arc::clone(&candidate_moves);
    let target = string_target()
        .on_will_accept(|details| details.data == "candidate")
        .on_move(move |details| {
            candidate_moves_for_cb
                .lock()
                .expect("not poisoned")
                .push(details.data);
        });
    let mut state = target.create_state();

    let candidate = pointer(1);
    let rejected = pointer(2);
    assert!(state.did_enter(&target, candidate, erase("candidate".to_string()), origin()));
    assert!(!state.did_enter(&target, rejected, erase("rejected".to_string()), origin()));

    state.did_move(&target, candidate, origin());
    state.did_move(&target, rejected, origin());

    assert_eq!(
        *candidate_moves.lock().expect("not poisoned"),
        vec!["candidate".to_string(), "rejected".to_string()],
        "on_move must fire for the rejected entry too, not just the candidate"
    );
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
