//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/draggable_test.dart`
//! (tag `3.44.0`, **71** `testWidgets` cases — `grep -cE '^\s*testWidgets\('`
//! against the tagged file).
//!
//! ### What this file proves
//!
//! Live drag-target discovery is wired: `DragTarget` publishes an
//! `Arc<DragTargetSlot>` as its hit-test payload (through `MetaData`), and a
//! dragging `Draggable` re-tests at the pointer's current global position on
//! every move and drives the enter/move/leave/drop transitions from the
//! result — the oracle's `_DragAvatar.updateDrag` / `_getDragTargets` shape.
//! The one divergence is where that global position comes from: FLUI's pointer
//! dispatch rewrites each event's position into the receiving node's LOCAL
//! space, so `Draggable` mounts a private origin probe under its `Listener`
//! and converts through `PipelineOwner::local_to_global`. See
//! `crates/flui-widgets/src/interaction/draggable.rs`'s divergence #2 and
//! `crates/flui-widgets/ARCHITECTURE.md`'s `## Mapping decisions`.
//!
//! The cases below fall into four groups:
//!
//! 1. **`Draggable`'s gesture lifecycle** — genuine pointer dispatch through
//!    the canonical `LaidOut` presentation harness, exercising the real
//!    `MultiDragGestureRecognizer`: start/update/end/cancel, the
//!    `child`/`child_when_dragging` swap, `max_simultaneous_drags`, axis
//!    restriction (including the oracle's "only fires when the restricted
//!    position actually moves" gate), and the divergent-but-documented
//!    immediate-cancel-on-unmount behavior.
//! 2. **One target's accept/candidate/reject/leave protocol** — driven
//!    directly against its `DragTargetSlot`, the same object a live drag
//!    drives it through, so a single transition is isolated from the
//!    sequencing rules that decide when a drag issues it.
//! 3. **The feedback layer's insert/reposition/remove mechanism** — `feedback`
//!    genuinely mounts as an `OverlayEntry` in a real ancestor `Overlay`
//!    (provided here by wrapping the draggable's route content in a
//!    `Navigator`, the same way a real app gets one), follows the tracked
//!    pointer displacement, and is removed on end/cancel/unmount. What is
//!    **not** proven is pixel-exact parity with the oracle's feedback
//!    *position* — see the first corpus bucket below.
//! 4. **Live discovery end to end** — a drag started clear of every target
//!    moving onto one, nested and overlapping targets, a transformed target's
//!    local position, an accepted drop, a target removed mid-drag (and a drop
//!    on the slot it left behind), the drag's own feedback not hiding the
//!    target beneath it, and the payload/`feedback_offset` snapshot a rebuild
//!    mid-drag must not disturb. The two cases a mounted harness cannot reach
//!    — a render tree that reports itself BUSY, and two genuinely concurrent
//!    contacts — are covered by unit tests in `draggable.rs` against a
//!    scripted probe.
//!
//! ### Denominator: 71 oracle cases
//!
//! **35 tests ported below** (11 `Draggable`-gesture + 6 slot-protocol + 6
//! feedback-layer-mechanism + 12 live-discovery, listed in each section's own
//! comment), plus 2 unit tests in `draggable.rs` for the two cases a mounted
//! harness cannot reach. Of these, 14 correspond exactly (or as an
//! explicitly-noted partial) to a specific oracle `testWidgets` name:
//! - `'Null axis onDragUpdate called only if draggable moves in any
//!   direction'`, `'Vertical axis onDragUpdate only called if draggable
//!   moves vertical'`, `'Horizontal axis onDragUpdate only called if
//!   draggable moves horizontal'` → the three `*_on_drag_update_only_fires_*`
//!   tests.
//! - `'Drag and drop - maxSimultaneousDrags'` → `max_simultaneous_drags_zero_disables_dragging`
//!   covers only the `maxSimultaneousDrags: 0` half of that oracle case; the
//!   `maxSimultaneousDrags: 2`-with-3-concurrent-pointers half needs two (or
//!   three) *independently addressable* concurrent contacts, and
//!   `LaidOut`'s `dispatch_pointer_*` sugar tracks exactly one
//!   "current contact" at a time (each `dispatch_pointer_down` reassigns it) —
//!   there is no way through this harness's public surface to `moveTo`/`up`
//!   an *earlier* contact once a later one has gone down. Named harness
//!   limitation, not silently dropped.
//! - `'Draggable disposes recognizer'` / `'Drag and drop - remove
//!   draggable'` → `unmounting_mid_drag_cancels_immediately_and_fires_end_and_canceled`
//!   proves the *documented divergence* (`draggable.rs`'s divergence #5)
//!   in their place, not the oracle's own keep-alive behavior.
//! - `'control test'` → `a_drop_over_an_accepting_target_completes_the_drag`.
//! - `'onLeave callback fires correctly'` (×2, with/without generic param) and
//!   `'onMove callback fires correctly'` (×2, ditto) →
//!   `a_drag_enters_a_target_it_moves_over_and_leaves_it_on_the_way_out`,
//!   which drives both through a live drag rather than the protocol alone.
//! - `'onDragCompleted called if dropped on accepting target'`, `'onDragEnd
//!   called if dropped on accepting target'`, and `'onDraggableCanceled not
//!   called if dropped on a accepting target'` →
//!   `a_drop_over_an_accepting_target_completes_the_drag`'s three outcome
//!   assertions.
//! - `'DragTarget does not call onDragEnd when remove from the tree'` →
//!   `a_target_removed_mid_drag_receives_nothing_further_and_accepts_nothing`,
//!   as a **partial**: it pins that the removed target receives nothing and
//!   the drop is not reported accepted, not the oracle's own `onDragEnd`
//!   suppression (this port fires `on_drag_end` unconditionally — see
//!   `pointer_cancel_fires_drag_end_before_canceled_with_zero_velocity`).
//!
//! The remaining 17 ported tests exercise the oracle's established
//! start/update/end/cancel/accept/candidate/reject/leave *contract*, plus the
//! feedback-layer mechanism — spread by the oracle across many live
//! end-to-end `testWidgets` cases — without a single corresponding
//! `testWidgets` name, same as the original cut of this file. (This includes
//! `reported_offset_is_displacement_not_global_position`, which pins a
//! *divergence* — see `draggable.rs`'s divergence note #4 — rather than
//! porting any single oracle case; the six feedback-layer tests are the
//! same shape, proving the mechanism the corpus's first bucket names as
//! still not enough to satisfy any of its 15 oracle cases.)
//!
//! **57 cases out of scope, with reasons (not silently dropped from the
//! count):** 15 + 20 + 12 + 3 + 3 + 1 + 1 + 1 + 1 = 57; together with the 14
//! in-scope oracle names above, that accounts for all 71.
//!
//! - **Feedback overlay presence/position (15 cases) — still 0 ported, for a
//!   *different* reason than before.** ADR-0036 closed the `Overlay.of`
//!   lookup gap `draggable.rs`'s divergence #1 used to name in full, and
//!   `feedback` now genuinely paints (see group 3 above and the
//!   `feedback_layer_*` tests below) — but every one of these 15 cases
//!   additionally needs something still missing: the five `'.../axis
//!   draggable moves ...'` cases and `'Drag feedback with child anchor
//!   positions correctly'` need the oracle's true global-position anchor
//!   (`dragAnchorStrategy` plus the real pointer/render-object global origin —
//!   divergence #4, separately pinned by
//!   `reported_offset_is_displacement_not_global_position`); `'... within a
//!   non-global Overlay ...'` and `'Drag feedback is put on root overlay with
//!   [rootOverlay] flag'` (×2, duplicate oracle name) need `rootOverlay`
//!   (an explicit ADR-0036 deferral); `'... matches pointer in scaled
//!   MaterialApp'`, `'childDragAnchorStrategy works in scaled MaterialApp'`,
//!   `'... matches pointer in rotated MaterialApp'` need transform-aware
//!   global positioning on top of the anchor-strategy gap; `'unmounting
//!   overlay ends drag gracefully'` needs the `Overlay` itself to unmount
//!   mid-drag, which this port's harness never drives (the `Overlay` is
//!   always kept alive for a test's duration) — `feedback_layer_is_removed_when_the_draggable_unmounts_mid_drag`
//!   below proves the narrower, Draggable-side half of the same shape (the
//!   `Draggable` unmounting while its `Overlay` stays up), not the oracle's
//!   own scenario, so the named case still doesn't port; `'feedback respect
//!   the MouseRegion cursor configure'` and `'configurable feedback ignore
//!   pointer behavior'` need cursor/hit-test configurability on the feedback
//!   layer, not implemented. None of the 15 has no remaining blocker, so none
//!   ports — but the mechanism all 15 implicitly depend on (an entry that
//!   inserts, repositions, and removes) is exactly what group 3's
//!   `feedback_layer_*` tests below now prove directly, the same way
//!   `reported_offset_is_displacement_not_global_position` proves the
//!   *shipped* semantics of a related, still-open gap rather than the
//!   oracle's own value.
//! - **Live drag-target discovery: 20 cases unported, but no longer
//!   blocked.** This bucket used to hold 28 cases with a hard blocker — no
//!   capability reachable from widget code to hit-test at a later, arbitrary
//!   position. That capability exists now (see "What this file proves"), and 8
//!   of the 28 are ported above. The other 20 are simply not written yet, with
//!   no remaining architectural obstacle for most of them: `'dragging over
//!   button'`, `'tapping button'`, `'horizontal and vertical draggables in
//!   vertical/horizontal block'` (×2), `'onDraggableCanceled called if dropped
//!   on non-accepting target'` (plus its `'...with details'` /
//!   `'...with correct velocity'` variants, ×2 more), `'onDragEnd not called
//!   if dropped on non-accepting target'` (+`'...with details'`, ×2),
//!   `'DragTarget rebuilds with and without rejected data ...'`, `'Can drag
//!   and drop over a non-accepting target multiple times'`, `'onDragCompleted
//!   not called if dropped on non-accepting target'` (+`'...with details'`,
//!   ×2), `'allow pass through of unaccepted data test'` (+`'...twice test'`,
//!   ×2), `'Draggable plays nice with onTap'`. Two keep their own, independent
//!   blockers: `'onMove is not called if moved with null data'` and
//!   `'onAccept is not called if dropped with null data'` need null-data
//!   modelling (`ErasedDragData` erases a concrete value, not an `Option`, so
//!   a genuinely-null `Draggable::data` has no representation — a drag without
//!   data discovers nothing here, where the oracle's enters every target). One
//!   is structurally not applicable: `'DragTarget does not set state when
//!   remove from the tree'` is a `setState`-after-dispose class of bug Rust's
//!   ownership model rules out — same reasoning as `dismissible_test.rs`'s
//!   `'Verify that drag-move events do not assert'` note.
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
use flui_rendering::hit_testing::HitTestBehavior;
use flui_types::layout::Axis;
use flui_types::{Color, Offset, Point, geometry::px};
use flui_view::{IntoView, StatefulView, ViewExt, ViewState};
use flui_widgets::{
    ColoredBox, DragPosition, DragTarget, DragTargetSlot, Draggable, DraggableDetails,
    ErasedDragData, Listener, Navigator, NavigatorHandle, Padding, Positioned, SimpleRoute,
    SizedBox, Stack, StackFit, Transform,
};

use crate::common::{LaidOut, lay_out, offset, tight};

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
fn settle_one_frame(scoped: &mut LaidOut) {
    scoped.pump_for(Duration::from_millis(1));
}

fn pointer(n: u64) -> PointerId {
    PointerId::new(n).expect("contact ids start at 1")
}

fn origin() -> Offset {
    Offset::new(px(0.0), px(0.0))
}

/// The screen origin, with no transform between the target and the root — the
/// position a protocol-level test hands a slot when the geometry is not what
/// it is exercising.
fn at_origin() -> DragPosition {
    DragPosition::global_only(origin())
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
// 11 cases: a lone immediate drag starts once on default arena victory; update reports the RAW delta
// (unrestricted — see the axis-gate cases below); the reported offset is
// displacement-since-drag-start, not the oracle's globally-anchored value
// (a pinned, named divergence — see `draggable.rs`'s divergence note #4 —
// exercised under a nonzero ancestor `Padding` so it is not origin-hidden);
// the null/vertical/horizontal axis "onDragUpdate only fires if the
// restricted position actually moved" gate (the oracle's own three-case
// group, ported 1:1); a drop over nothing reports unaccepted and fires
// canceled, never completed, with the tracked displacement offset; a platform pointer-cancel
// also fires `on_drag_end` with that same offset and zero velocity
// (Flutter's `finishDrag` is unconditional — this project found and fixed a
// divergence from that while building this port, see
// `pointer_cancel_fires_drag_end_before_canceled_with_zero_velocity` below);
// `max_simultaneous_drags(0)` disables dragging entirely; `child_when_dragging`
// swaps in while active and reverts after the drag ends; unmounting mid-drag
// cancels immediately (the documented divergence from the oracle's
// recognizer keep-alive).

#[test]
fn lone_immediate_drag_starts_once_on_default_arena_victory() {
    let started = Arc::new(AtomicUsize::new(0));
    let started_for_cb = Arc::clone(&started);
    let widget = Draggable::<i32>::new(child())
        .data(1)
        .on_drag_started(move || {
            started_for_cb.fetch_add(1, Ordering::SeqCst);
        });
    let scoped = lay_out(widget, extent());

    scoped.dispatch_pointer_down(50.0, 50.0);
    assert_eq!(
        started.load(Ordering::SeqCst),
        1,
        "a lone ImmediateMultiDrag recognizer wins the deferred default after Down"
    );

    scoped.dispatch_pointer_move(75.0, 50.0);
    assert_eq!(
        started.load(Ordering::SeqCst),
        1,
        "movement within the same contact must not restart the drag"
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
    let scoped = lay_out(widget, extent());

    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_move(75.0, 50.0); // first post-start update
    scoped.dispatch_pointer_move(95.0, 50.0); // the real update: +20px horizontal

    let delta = last_delta
        .lock()
        .expect("not poisoned")
        .expect("on_drag_update fired after pointer movement");
    assert!(
        (delta.dx.0 - 20.0).abs() < 0.01,
        "expected a +20px horizontal delta, got {delta:?}"
    );
    scoped.dispatch_pointer_up(95.0, 50.0);
}

#[test]
fn reported_offset_is_displacement_not_global_position() {
    // Pins the shipped semantics `draggable.rs`'s divergence note #4 names:
    // `DraggableDetails.offset` is displacement-since-drag-start (the
    // running sum of axis-restricted deltas), NOT the oracle's
    // `_lastOffset` (which adds the draggable's global origin on top of
    // that same sum). The `Draggable` here sits under a nonzero `Padding`
    // — its own render object's global origin is `(60, 40)`, not `(0, 0)`
    // — specifically so this test is NOT satisfied by an implementation
    // that happens to be correct only at the screen origin: if a future
    // change seeded the tracked offset with *any* nonzero base (an
    // attempted, incomplete "fix" toward the oracle's real semantics), the
    // reported value below would shift away from the exact displacement
    // and this assertion would catch it.
    use flui_types::geometry::EdgeInsets;
    use flui_widgets::Padding;

    let end_details: Arc<StdMutex<Option<DraggableDetails>>> = Arc::new(StdMutex::new(None));
    let end_for_cb = Arc::clone(&end_details);
    let widget = Draggable::<i32>::new(child()).on_drag_end(move |details| {
        *end_for_cb.lock().expect("not poisoned") = Some(details);
    });
    let padded = Padding::new(EdgeInsets::new(px(40.0), px(0.0), px(0.0), px(60.0))).child(widget);
    let mut scoped = lay_out(padded, tight(300.0, 300.0));

    // (70, 50) is 10px inside the padded Draggable's own top-left (60, 40).
    scoped.dispatch_pointer_down(70.0, 50.0);
    scoped.dispatch_pointer_move(100.0, 50.0); // +30px horizontal, +0 vertical
    scoped.dispatch_pointer_up(100.0, 50.0);
    settle_one_frame(&mut scoped);

    let details = end_details
        .lock()
        .expect("not poisoned")
        .expect("on_drag_end fired");
    assert!(
        (details.offset.dx.0 - 30.0).abs() < 0.01 && details.offset.dy.0.abs() < 0.01,
        "offset must be the raw +30px displacement regardless of the \
         Padding's (60, 40) origin — got {:?}",
        details.offset
    );
}

/// Records every `on_drag_update` delta and a running fire count.
#[derive(Default)]
struct UpdateLog {
    fires: usize,
    last_delta: Option<Offset<flui_types::geometry::PixelDelta>>,
}

// A lone immediate recognizer has already won the deferred arena default after
// Down. Every case below therefore uses one clean, nonzero post-start move so
// the assertions isolate Draggable's axis restriction rather than gesture
// acceptance or pending-delta accumulation.

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
    let scoped = lay_out(widget, large_extent());

    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_move(80.0, 80.0); // +30,+30: first nonzero update
    assert_eq!(
        log.lock().expect("not poisoned").fires,
        1,
        "the first nonzero post-start move must fire"
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
    let scoped = lay_out(widget, large_extent());

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
    let scoped = lay_out(widget, large_extent());

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

/// A drop with no target anywhere under it is unaccepted: `on_drag_end`
/// reports so, `on_draggable_canceled` fires, `on_drag_completed` does not.
///
/// The complement — a drop that IS over a target — is
/// `a_drop_over_an_accepting_target_completes_the_drag` in group 4. Both are
/// needed: this one alone would also pass against a `Draggable` that never
/// discovered anything.
#[test]
fn a_drop_over_no_target_reports_unaccepted_and_fires_canceled() {
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
    let mut scoped = lay_out(widget, extent());

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
        "a drop with nothing under it is not accepted by anything"
    );
    assert_eq!(
        canceled.load(Ordering::SeqCst),
        1,
        "on_draggable_canceled fires for every unaccepted drop"
    );
    assert_eq!(
        completed.load(Ordering::SeqCst),
        0,
        "on_drag_completed must not fire for a drop no target took"
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
            // The tracked displacement offset (25px right, matching the
            // move below), not zero — only velocity is zero on cancel.
            assert!((details.offset.dx.0 - 25.0).abs() < 0.01);
            end_for_cb.fetch_add(1, Ordering::SeqCst);
        })
        .on_draggable_canceled(move |velocity, offset| {
            assert_eq!(velocity.pixels_per_second, origin());
            assert!((offset.dx.0 - 25.0).abs() < 0.01);
            canceled_for_cb.fetch_add(1, Ordering::SeqCst);
        });
    let scoped = lay_out(widget, extent());

    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_move(75.0, 50.0);
    scoped.dispatch_pointer_cancel();

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
    let scoped = lay_out(widget, extent());

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
    let mut scoped = lay_out(widget, extent());

    assert_eq!(
        scoped.find_all_by_render_type("RenderConstrainedBox").len(),
        0,
        "at rest, `child_when_dragging`'s wrapper must not be mounted"
    );

    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_move(80.0, 50.0);
    settle_one_frame(&mut scoped);

    assert_eq!(
        scoped.find_all_by_render_type("RenderConstrainedBox").len(),
        1,
        "mid-drag, `child_when_dragging` (a SizedBox) is mounted"
    );

    scoped.dispatch_pointer_up(80.0, 50.0);
    settle_one_frame(&mut scoped);

    assert_eq!(
        scoped.find_all_by_render_type("RenderConstrainedBox").len(),
        0,
        "after the drag ends, `child_when_dragging` is unmounted again and `child` is back"
    );
    assert_eq!(
        scoped.find_all_by_render_type("RenderDecoratedBox").len(),
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
    // the arena-driven dispatch on the canonical `LaidOut`
    // provides (plain `lay_out`'s dispatch never starts the drag at all —
    // confirmed directly while building this case). But swapping the
    // *literal root element's own type* via `pump_widget`/`swap_root_view`
    // does not run the normal unmount/dispose path either (also confirmed
    // directly: a bare-root swap never calls `DraggableState::dispose`, with
    // or without an active drag) — `dismissible_test.rs`'s own unmount cases
    // hit the same thing and avoid it by keeping an outer wrapper (there,
    // `VsyncScope`) stable and swapping only its child. Here,
    // `LaidOut::pump_widget` preserves the harness-owned
    // `GestureArenaScope`, while Padding keeps the application-side root
    // type stable.
    //
    // The stable wrapper is `Padding::new(EdgeInsets::ZERO)`, not `Center`:
    // `Center` gives its child its own preferred (loose) size, which shrank
    // `Draggable`'s `ColoredBox` away from the coordinates this test dispatches
    // to (confirmed directly — the drag never started under `Center`). Zero
    // padding passes the incoming tight `extent()` constraint straight
    // through, so `child()`'s `ColoredBox` still fills the whole box exactly
    // as it does everywhere else in this file.
    use flui_types::geometry::EdgeInsets;
    use flui_widgets::Padding;

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
    let mut scoped = lay_out(Padding::new(EdgeInsets::ZERO).child(widget), extent());

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
    // while the drag is still active. The scoped harness preserves the
    // GestureArenaScope root; only Padding's child changes.
    scoped.pump_widget(Padding::new(EdgeInsets::ZERO).child(child()));

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
// 6 cases, driven directly against one target's `DragTargetSlot` — the same
// object a live drag drives it through, one layer below the discovery a
// `Draggable` performs (group 4 covers that end to end). Driving the slot
// directly is how a single transition is isolated from the sequencing rules
// that decide *when* a drag issues it: data delivered on an accepted drop;
// the candidate list
// gains/loses entries across enter/leave; `on_will_accept` returning `false`
// routes to the rejected list instead of the candidate list; typed-data
// mismatch (a `DragTarget<String>` given an `i32` payload) never becomes an
// entry at all, matching the oracle's discovery-time `isExpectedDataType`
// filter; `did_move` fires for a rejected entry too — the oracle's own
// `didMove` gates only on null data, not rejection status; `did_drop` only
// accepts a *current* candidate.

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
    let state = target.create_state();
    let slot = state.slot();

    let p = pointer(1);
    assert!(slot.did_enter(p, &erase("hello".to_string()), at_origin()));
    assert!(slot.did_drop(p, at_origin()));

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
    let state = target.create_state();
    let slot = state.slot();
    let p = pointer(1);

    assert!(state.candidate_data().is_empty());
    assert!(slot.did_enter(p, &erase("a".to_string()), at_origin()));
    assert_eq!(
        state.candidate_data(),
        vec![Some("a".to_string())],
        "an accepted enter must appear in the candidate list"
    );

    slot.did_leave(p);
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
    let state = target.create_state();
    let slot = state.slot();
    let p = pointer(1);

    let accepted = slot.did_enter(p, &erase("a".to_string()), at_origin());

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

    slot.did_leave(p);
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
    let state = target.create_state();
    let slot = state.slot();
    let p = pointer(1);

    let accepted = slot.did_enter(p, &erase(42_i32), at_origin());

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
    slot.did_leave(p);
    slot.did_move(p, at_origin());
    assert!(!slot.did_drop(p, at_origin()));
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
    let state = target.create_state();
    let slot = state.slot();

    let candidate = pointer(1);
    let rejected = pointer(2);
    assert!(slot.did_enter(candidate, &erase("candidate".to_string()), at_origin()));
    assert!(!slot.did_enter(rejected, &erase("rejected".to_string()), at_origin()));

    slot.did_move(candidate, at_origin());
    slot.did_move(rejected, at_origin());

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
    let state = target.create_state();
    let slot = state.slot();
    let p = pointer(1);

    assert!(!slot.did_enter(p, &erase("a".to_string()), at_origin()));
    let dropped = slot.did_drop(p, at_origin());

    assert!(!dropped, "a rejected pointer's drop must be a no-op");
    assert_eq!(
        accepted.load(Ordering::SeqCst),
        0,
        "on_accept must not fire for a drop that was never a candidate"
    );

    // An unknown pointer (never entered at all) is equally a no-op.
    assert!(!slot.did_drop(pointer(2), at_origin()));
}

// ============================================================================
// Group 3 — the feedback layer's insert/reposition/remove mechanism
// ============================================================================
//
// 6 cases: `feedback` mounts once the drag starts and is gone once it ends
// (via a real `pointer up`) or is cancelled (via a platform `pointer
// cancel`); it repositions to follow the tracked displacement across
// multiple moves; `child_when_dragging`'s swap keeps working unchanged
// alongside it; the layer is torn down if the `Draggable` itself unmounts
// mid-drag (the route holding it is replaced), not just on a normal
// end/cancel; and a rapid restart (a new drag starting before an ended
// one's removal has drained through a rebuild) does not leave a stale,
// frozen layer stuck forever while the new drag shows none of its own. None
// of these correspond to a specific oracle `testWidgets` name — see the
// corpus accounting's first bucket above for why the 15 named
// feedback-position cases still do not port even though this mechanism is
// now real (each needs something else still missing: the true global-anchor
// position, `rootOverlay`, scaled/rotated ancestors, or cursor
// configurability).
//
// `Overlay::maybe_of` needs a real ancestor `Overlay` to find — provided
// here by wrapping the draggable in a `Navigator`'s route content, the
// public way any real app gets one (`Navigator`'s own privately-held
// `Overlay` is not otherwise constructible outside `flui-widgets`).

/// A `Draggable` mounted as a `Navigator`'s sole route's content, so
/// `Overlay::maybe_of` resolves the `Overlay` that `Navigator::build` mounts.
fn lay_out_draggable_with_overlay(widget: Draggable<i32>, constraints: BoxConstraints) -> LaidOut {
    let handle = NavigatorHandle::new();
    handle.seed_initial(SimpleRoute::<i32>::new(move |_ctx| widget.clone().boxed()));
    lay_out(Navigator::new(handle), constraints)
}

/// The top-left corner of the one mounted `RenderConstrainedBox` (this
/// group's feedback content, a bare `SizedBox`) — `find_all_by_render_type`
/// is exact-type, not a substring match, so this does not also match
/// `child()`'s `ColoredBox` (`RenderDecoratedBox`).
fn feedback_origin(scoped: &LaidOut) -> Point {
    let matches = scoped.find_all_by_render_type("RenderConstrainedBox");
    let target = *matches
        .first()
        .expect("the feedback layer's SizedBox must be mounted");
    scoped
        .pipeline_owner()
        .with(|owner| owner.local_to_global(target, Point::ZERO, None))
        .expect("committed layout")
}

#[test]
fn feedback_layer_appears_on_start_and_disappears_on_end() {
    let widget = Draggable::<i32>::new(child()).feedback(|| SizedBox::new(20.0, 10.0).boxed());
    let mut scoped = lay_out_draggable_with_overlay(widget, extent());

    assert_eq!(
        scoped.find_all_by_render_type("RenderConstrainedBox").len(),
        0,
        "no feedback layer before the drag starts"
    );

    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_move(80.0, 50.0); // crosses the slop: starts the drag
    settle_one_frame(&mut scoped);
    assert_eq!(
        scoped.find_all_by_render_type("RenderConstrainedBox").len(),
        1,
        "the feedback layer mounts once the drag starts"
    );

    scoped.dispatch_pointer_up(80.0, 50.0);
    // Two ticks: the first drains `DraggableState::build`'s own rebuild
    // (`end_active`'s `rebuild.schedule()`), which is where `entry.remove()`
    // runs; removal itself schedules a *second*, separate rebuild — of the
    // `Overlay`, a different element — so the entry's element does not
    // actually leave the render tree until that one drains too. Same
    // two-hop shape `overlay::tests::overlay_remove_entry_rebuilds` pins
    // directly for `OverlayEntry::remove` in isolation.
    settle_one_frame(&mut scoped);
    settle_one_frame(&mut scoped);
    assert_eq!(
        scoped.find_all_by_render_type("RenderConstrainedBox").len(),
        0,
        "the feedback layer is removed once the drag ends"
    );
}

#[test]
fn feedback_layer_is_removed_on_cancel_too() {
    // Flutter's `_DragAvatar.finishDrag` (which both `endDrag` and
    // `cancelDrag` route through) removes `_overlayEntry` unconditionally —
    // a platform cancel must tear the feedback layer down exactly like a
    // real pointer-up does.
    let widget = Draggable::<i32>::new(child()).feedback(|| SizedBox::new(20.0, 10.0).boxed());
    let mut scoped = lay_out_draggable_with_overlay(widget, extent());

    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_move(80.0, 50.0);
    settle_one_frame(&mut scoped);
    assert_eq!(
        scoped.find_all_by_render_type("RenderConstrainedBox").len(),
        1,
        "the feedback layer is showing mid-drag"
    );

    scoped.dispatch_pointer_cancel();
    // Two ticks: see `feedback_layer_appears_on_start_and_disappears_on_end`'s
    // comment on why `entry.remove()` needs a second, separate drain.
    settle_one_frame(&mut scoped);
    settle_one_frame(&mut scoped);
    assert_eq!(
        scoped.find_all_by_render_type("RenderConstrainedBox").len(),
        0,
        "a platform cancel must remove the feedback layer exactly like a real pointer-up"
    );
}

#[test]
fn feedback_layer_follows_pointer_moves() {
    let widget = Draggable::<i32>::new(child())
        .feedback(|| SizedBox::new(20.0, 10.0).boxed())
        .feedback_offset(Offset::new(px(5.0), px(7.0)));
    let mut scoped = lay_out_draggable_with_overlay(widget, large_extent());

    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_move(80.0, 50.0); // +30 horizontal: starts the drag
    settle_one_frame(&mut scoped);
    let first = feedback_origin(&scoped);

    scoped.dispatch_pointer_move(80.0, 90.0); // +0 horizontal, +40 vertical from here
    settle_one_frame(&mut scoped);
    let second = feedback_origin(&scoped);

    assert!(
        (second.y.0 - first.y.0 - 40.0).abs() < 0.5,
        "the feedback layer must move by the same vertical delta as the \
         pointer: first={first:?} second={second:?}"
    );
    assert!(
        (second.x.0 - first.x.0).abs() < 0.5,
        "a purely-vertical move must not shift the feedback layer \
         horizontally: first={first:?} second={second:?}"
    );

    scoped.dispatch_pointer_up(80.0, 90.0);
}

#[test]
fn feedback_layer_and_child_when_dragging_swap_together() {
    // Cheap co-assertion (task-requested, not a new mechanism): the
    // pre-existing `child`/`child_when_dragging` swap and the new feedback
    // layer are both driven by the same `active_count` transition and must
    // not interfere with each other.
    let widget = Draggable::<i32>::new(child())
        .child_when_dragging(|| {
            SizedBox::new(100.0, 100.0)
                .child(ColoredBox::new(Color::rgb(90, 90, 90)))
                .boxed()
        })
        .feedback(|| ColoredBox::new(Color::rgb(10, 200, 10)).boxed());
    let mut scoped = lay_out_draggable_with_overlay(widget, extent());

    assert_eq!(
        scoped.find_all_by_render_type("RenderDecoratedBox").len(),
        1,
        "at rest: just `child`'s ColoredBox"
    );
    assert_eq!(
        scoped.find_all_by_render_type("RenderConstrainedBox").len(),
        0,
        "at rest: no child_when_dragging wrapper"
    );

    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_move(80.0, 50.0);
    settle_one_frame(&mut scoped);

    assert_eq!(
        scoped.find_all_by_render_type("RenderDecoratedBox").len(),
        2,
        "mid-drag: child_when_dragging's inner ColoredBox + the feedback layer's ColoredBox"
    );
    assert_eq!(
        scoped.find_all_by_render_type("RenderConstrainedBox").len(),
        1,
        "mid-drag: child_when_dragging's own SizedBox wrapper"
    );

    scoped.dispatch_pointer_up(80.0, 50.0);
    // Two ticks: see `feedback_layer_appears_on_start_and_disappears_on_end`'s
    // comment on why `entry.remove()` needs a second, separate drain.
    settle_one_frame(&mut scoped);
    settle_one_frame(&mut scoped);

    assert_eq!(
        scoped.find_all_by_render_type("RenderDecoratedBox").len(),
        1,
        "after the drag ends: back to just `child`"
    );
    assert_eq!(
        scoped.find_all_by_render_type("RenderConstrainedBox").len(),
        0,
        "after the drag ends: child_when_dragging and the feedback layer are both gone"
    );
}

/// Narrower cousin of the oracle's `'unmounting overlay ends drag
/// gracefully'` (one of the 15 out-of-scope feedback-position cases, see the
/// corpus accounting above): this port's harness always keeps the `Overlay`
/// itself mounted for a test's duration, so the oracle's exact scenario (the
/// `Overlay` unmounting) isn't driven here. What this proves instead is the
/// `Draggable` side of the same shape — its own `dispose` tearing the
/// feedback layer down when *it* unmounts mid-drag (the route holding it is
/// replaced), not just when the drag ends normally.
///
/// Red-check: comment out `dispose`'s stale-take-and-remove in
/// `draggable.rs` — this test's second assertion then fails (the entry is
/// orphaned in the overlay forever, since no later `build` runs for a
/// disposed element to catch it there either).
#[test]
fn feedback_layer_is_removed_when_the_draggable_unmounts_mid_drag() {
    let widget = Draggable::<i32>::new(child()).feedback(|| SizedBox::new(20.0, 10.0).boxed());
    let handle = NavigatorHandle::new();
    handle.seed_initial(SimpleRoute::<i32>::new(move |_ctx| widget.clone().boxed()));
    let mut scoped = lay_out(Navigator::new(handle.clone()), extent());

    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_move(80.0, 50.0);
    settle_one_frame(&mut scoped);
    assert_eq!(
        scoped.find_all_by_render_type("RenderConstrainedBox").len(),
        1,
        "the feedback layer is showing mid-drag"
    );

    // Replace the route holding the Draggable while the drag is still
    // active — this tears down its subtree, including `DraggableState`,
    // without ever going through a normal `pointer up`/`cancel`. The
    // replacement's own content is a `ColoredBox` (`RenderDecoratedBox`),
    // deliberately *not* a `SizedBox` (`RenderConstrainedBox`) — the latter
    // would collide with the feedback layer's own marker type below and
    // make a bug that leaves the old entry mounted look like a pass.
    handle.push_replacement(SimpleRoute::<i32>::new(|_ctx| {
        ColoredBox::new(Color::rgb(0, 0, 0)).boxed()
    }));
    // Four hops, each needing its own drain: (1) Navigator's own rebuild
    // calls `overlay.rearrange`, which (2) schedules the Overlay's own
    // rebuild, unmounting the old route's subtree and calling
    // `DraggableState::dispose` — which itself calls `entry.remove()`, (3)
    // scheduling a third rebuild that detaches the feedback entry from the
    // overlay's list, which (4) finally drops it from the render tree.
    settle_one_frame(&mut scoped);
    settle_one_frame(&mut scoped);
    settle_one_frame(&mut scoped);

    assert_eq!(
        scoped.find_all_by_render_type("RenderConstrainedBox").len(),
        0,
        "the feedback layer must not outlive the Draggable that inserted it"
    );
}

/// A rapid restart at `max_simultaneous_drags(1)` — a new drag starting
/// before the ended one's `entry.remove()` has drained through a rebuild —
/// must not leave the old, now-frozen feedback layer stuck in the overlay
/// forever with the new drag showing none of its own.
///
/// A plain render-object *count* cannot tell "the new drag's own live
/// feedback" apart from "the old drag's orphaned, frozen-in-place feedback"
/// — both leave exactly one `RenderConstrainedBox` mounted. So this checks
/// *position*: the first drag moves purely horizontally, the second (started
/// before the first's removal drains) purely vertically. If the slot still
/// held the first drag's stale, un-evicted entry, nothing would ever be
/// writing to *its* `FeedbackSignal` again (a stale entry has no live
/// session), so it would stay frozen at the first drag's horizontal-only
/// position — the second drag's own vertical move would not move it, because
/// `on_start` would have handed that second session `feedback: None`.
///
/// Red-check: revert `on_start` to only act "if slot.is_none()" (the
/// pre-fix shape) — the entry visible after the second drag's move stays
/// frozen at the first drag's horizontal-only position (this test's second
/// assertion fails).
#[test]
fn feedback_layer_survives_a_restart_before_the_previous_removal_drains() {
    let widget = Draggable::<i32>::new(child())
        .max_simultaneous_drags(1)
        .feedback(|| SizedBox::new(20.0, 10.0).boxed());
    let mut scoped = lay_out_draggable_with_overlay(widget, extent());

    // First drag: +30 horizontal, 0 vertical.
    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_move(80.0, 50.0);
    settle_one_frame(&mut scoped);
    assert_eq!(
        scoped.find_all_by_render_type("RenderConstrainedBox").len(),
        1,
        "the first drag's feedback layer is showing"
    );

    // End the first drag and *immediately* start a second, with no
    // intervening `settle_one_frame` — `end_active`'s scheduled rebuild
    // (which is where the ended drag's stale entry would normally be
    // removed) has not drained yet when the new drag's own `on_start` runs.
    // Second drag: 0 horizontal, +40 vertical — the opposite axis, so a
    // frozen first-drag entry and a live second-drag entry are
    // unambiguously distinguishable by position.
    scoped.dispatch_pointer_up(80.0, 50.0);
    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_move(50.0, 90.0);
    settle_one_frame(&mut scoped);
    settle_one_frame(&mut scoped);

    assert_eq!(
        scoped.find_all_by_render_type("RenderConstrainedBox").len(),
        1,
        "exactly one feedback layer must remain — the new drag's own, not \
         the old drag's orphaned one left behind on top of it"
    );
    let position = feedback_origin(&scoped);
    assert!(
        (position.y.0 - 40.0).abs() < 0.5 && position.x.0.abs() < 0.5,
        "the surviving layer must be the second drag's own — tracking its \
         +40 vertical move, not frozen at the first drag's +30 horizontal \
         position — got {position:?}"
    );
}

// ============================================================================
// Group 4 — live drag-target discovery
// ============================================================================
//
// The oracle's `_DragAvatar.updateDrag` hit-tests at the pointer's CURRENT
// global position on every move and walks the result for `RenderMetaData`
// nodes whose payload is a drag target (`_getDragTargets`). These cases cover
// both halves of that in FLUI: the target publishing a findable payload, and
// the drag finding it and sequencing the transitions.

/// The target's own discovery edge: a mounted `DragTarget` must be findable
/// from a hit test over it, carrying the slot a drag drives.
///
/// Its content here is a bare `SizedBox`, which claims no hits of its own —
/// deliberately, because the oracle's default `hitTestBehavior` is
/// `translucent`, i.e. the target is found within its own bounds whatever its
/// content does. A `DeferToChild` tag over this child would find nothing.
#[test]
fn a_mounted_drag_target_publishes_a_slot_a_hit_test_can_find() {
    let laid = lay_out(
        DragTarget::<String>::new(|_candidates, _rejected| SizedBox::new(40.0, 40.0).boxed()),
        tight(40.0, 40.0),
    );

    let hit = laid.hit_test_pointer(offset(20.0, 20.0));

    assert!(
        !hit.path().is_empty(),
        "premise: the position must hit something, so a missing slot below \
         is about the payload and not about the hit"
    );
    let found = hit
        .path()
        .iter()
        .filter(|entry| entry.metadata_as::<DragTargetSlot>().is_some())
        .count();
    assert_eq!(
        found, 1,
        "a hit over a mounted DragTarget must carry its slot back out — \
         without it no drag can ever discover this target"
    );
}

/// A target that appends every transition it receives to `log`, tagged with
/// `name`, so a test can assert the exact ORDER of a boundary crossing rather
/// than only the final state.
///
/// `on_will_accept` doubles as the enter probe: it is the one callback the
/// oracle fires exactly once per `didEnter`.
fn logging_target(log: &Arc<StdMutex<Vec<String>>>, name: &'static str) -> DragTarget<String> {
    let enter_log = Arc::clone(log);
    let move_log = Arc::clone(log);
    let leave_log = Arc::clone(log);
    let accept_log = Arc::clone(log);
    DragTarget::<String>::new(|_candidates, _rejected| SizedBox::expand().boxed())
        .on_will_accept(move |_details| {
            enter_log
                .lock()
                .expect("not poisoned")
                .push(format!("enter {name}"));
            true
        })
        .on_move(move |_details| {
            move_log
                .lock()
                .expect("not poisoned")
                .push(format!("move {name}"));
        })
        .on_leave(move |_data| {
            leave_log
                .lock()
                .expect("not poisoned")
                .push(format!("leave {name}"));
        })
        .on_accept(move |details| {
            accept_log
                .lock()
                .expect("not poisoned")
                .push(format!("accept {name} {}", details.data));
        })
}

/// The draggable these cases pick up, carrying `"parcel"`.
fn parcel() -> Draggable<String> {
    Draggable::<String>::new(child()).data("parcel".to_string())
}

/// A 200x200 stack: `target_area` fills the 100x100 top-left corner, and the
/// draggable sits in a 40x40 box at (150, 150) — clear of it, so a drag can
/// start outside every target and move in.
///
/// The shape is fixed so a test can swap only `target_area` through
/// `pump_widget` and keep the draggable's own element (and its in-flight
/// drag) alive across the rebuild.
fn stack_root(target_area: impl IntoView, draggable: Draggable<String>) -> Stack {
    Stack::new(vec![
        Positioned::new(target_area)
            .left(0.0)
            .top(0.0)
            .width(100.0)
            .height(100.0)
            .into_view()
            .boxed(),
        Positioned::new(draggable)
            .left(150.0)
            .top(150.0)
            .width(40.0)
            .height(40.0)
            .into_view()
            .boxed(),
    ])
    .fit(StackFit::Expand)
}

fn stack_with(target_area: impl IntoView, draggable: Draggable<String>) -> LaidOut {
    lay_out(stack_root(target_area, draggable), tight(200.0, 200.0))
}

fn transitions(log: &Arc<StdMutex<Vec<String>>>) -> Vec<String> {
    log.lock().expect("not poisoned").clone()
}

/// A drag that starts clear of every target and moves onto one enters it,
/// and moving back out leaves it — once each, at the boundary.
///
/// This is the whole of `_DragAvatar.updateDrag` end to end: the pointer route
/// was resolved at Down over the *draggable*, so nothing about that route
/// mentions the target; only a fresh hit test at the moved-to position can
/// find it.
#[test]
fn a_drag_enters_a_target_it_moves_over_and_leaves_it_on_the_way_out() {
    let log = Arc::new(StdMutex::new(Vec::new()));
    let scoped = stack_with(logging_target(&log, "inbox"), parcel());

    scoped.dispatch_pointer_down(170.0, 170.0);
    scoped.dispatch_pointer_move(160.0, 160.0);
    assert_eq!(
        transitions(&log),
        Vec::<String>::new(),
        "a drag still clear of every target must not have entered one"
    );

    scoped.dispatch_pointer_move(50.0, 50.0);
    assert_eq!(
        transitions(&log),
        vec!["enter inbox".to_string(), "move inbox".to_string()],
        "moving onto the target must enter it, then report the move"
    );

    scoped.dispatch_pointer_move(45.0, 45.0);
    assert_eq!(
        transitions(&log),
        vec![
            "enter inbox".to_string(),
            "move inbox".to_string(),
            "move inbox".to_string()
        ],
        "a further move inside the same target must report a move, not \
         re-enter it"
    );

    scoped.dispatch_pointer_move(170.0, 170.0);
    assert_eq!(
        transitions(&log).last().map(String::as_str),
        Some("leave inbox"),
        "moving back out must leave the target exactly once"
    );
}

/// A drop over an accepting target delivers the data, reports the drag as
/// accepted, and fires `on_drag_completed` instead of `on_draggable_canceled`.
#[test]
fn a_drop_over_an_accepting_target_completes_the_drag() {
    let log = Arc::new(StdMutex::new(Vec::new()));
    let completed = Arc::new(AtomicUsize::new(0));
    let canceled = Arc::new(AtomicUsize::new(0));
    let accepted_flag = Arc::new(StdMutex::new(None));

    let completed_for_cb = Arc::clone(&completed);
    let canceled_for_cb = Arc::clone(&canceled);
    let accepted_for_cb = Arc::clone(&accepted_flag);
    let draggable = parcel()
        .on_drag_completed(move || {
            completed_for_cb.fetch_add(1, Ordering::SeqCst);
        })
        .on_draggable_canceled(move |_velocity, _offset| {
            canceled_for_cb.fetch_add(1, Ordering::SeqCst);
        })
        .on_drag_end(move |details| {
            *accepted_for_cb.lock().expect("not poisoned") = Some(details.was_accepted);
        });
    let scoped = stack_with(logging_target(&log, "inbox"), draggable);

    scoped.dispatch_pointer_down(170.0, 170.0);
    scoped.dispatch_pointer_move(50.0, 50.0);
    scoped.dispatch_pointer_up(50.0, 50.0);

    assert_eq!(
        transitions(&log).last().map(String::as_str),
        Some("accept inbox parcel"),
        "the drop must deliver the draggable's own data to the target"
    );
    assert_eq!(
        *accepted_flag.lock().expect("not poisoned"),
        Some(true),
        "DraggableDetails::was_accepted must report the accepted drop"
    );
    assert_eq!(
        completed.load(Ordering::SeqCst),
        1,
        "an accepted drop fires on_drag_completed"
    );
    assert_eq!(
        canceled.load(Ordering::SeqCst),
        0,
        "an accepted drop must not also report itself canceled"
    );
}

/// Nested targets: the innermost accepting one wins, and the outer target it
/// sits inside is never entered while it is active.
///
/// Flutter parity: `_DragAvatar.updateDrag`'s `firstWhere` over the leaf-first
/// path stops at the first target whose `didEnter` returns true, so targets
/// below (outside) it are shadowed by the acceptance.
#[test]
fn the_innermost_accepting_target_shadows_the_outer_one_it_sits_in() {
    let log = Arc::new(StdMutex::new(Vec::new()));
    let inner = logging_target(&log, "inner");
    let outer = DragTarget::<String>::new(move |_candidates, _rejected| {
        Padding::all(20.0).child(inner.clone()).boxed()
    });
    let outer = {
        let enter_log = Arc::clone(&log);
        let leave_log = Arc::clone(&log);
        let move_log = Arc::clone(&log);
        outer
            .on_will_accept(move |_details| {
                enter_log
                    .lock()
                    .expect("not poisoned")
                    .push("enter outer".to_string());
                true
            })
            .on_move(move |_details| {
                move_log
                    .lock()
                    .expect("not poisoned")
                    .push("move outer".to_string());
            })
            .on_leave(move |_data| {
                leave_log
                    .lock()
                    .expect("not poisoned")
                    .push("leave outer".to_string());
            })
    };
    let scoped = stack_with(outer, parcel());

    // (10, 10) is inside the outer 100x100 box but inside its 20px padding
    // ring, so only the outer target is under the pointer.
    scoped.dispatch_pointer_down(170.0, 170.0);
    scoped.dispatch_pointer_move(10.0, 10.0);
    assert_eq!(
        transitions(&log),
        vec!["enter outer".to_string(), "move outer".to_string()],
        "in the padding ring, only the outer target is hit"
    );

    // (50, 50) is inside both. Crossing that boundary leaves the outer and
    // enters the inner — and the outer, now shadowed, is not re-entered.
    log.lock().expect("not poisoned").clear();
    scoped.dispatch_pointer_move(50.0, 50.0);
    assert_eq!(
        transitions(&log),
        vec![
            "leave outer".to_string(),
            "enter inner".to_string(),
            "move inner".to_string()
        ],
        "crossing into the inner target must leave the outer, enter the \
         inner, and leave the outer un-entered underneath it"
    );
}

/// When the innermost target vetoes the drag, the search continues outwards:
/// the vetoing target is still entered (as a rejected one, receiving moves and
/// a leave), and the outer target below it becomes the active one.
#[test]
fn a_vetoing_inner_target_is_still_entered_and_the_outer_one_accepts() {
    let log = Arc::new(StdMutex::new(Vec::new()));
    let inner = {
        let enter_log = Arc::clone(&log);
        let move_log = Arc::clone(&log);
        DragTarget::<String>::new(|_candidates, _rejected| SizedBox::expand().boxed())
            .on_will_accept(move |_details| {
                enter_log
                    .lock()
                    .expect("not poisoned")
                    .push("enter inner".to_string());
                false
            })
            .on_move(move |_details| {
                move_log
                    .lock()
                    .expect("not poisoned")
                    .push("move inner".to_string());
            })
    };
    let accepted = Arc::new(StdMutex::new(None));
    let accepted_for_cb = Arc::clone(&accepted);
    let outer = {
        let enter_log = Arc::clone(&log);
        let move_log = Arc::clone(&log);
        DragTarget::<String>::new(move |_candidates, _rejected| {
            Padding::all(20.0).child(inner.clone()).boxed()
        })
        .on_will_accept(move |_details| {
            enter_log
                .lock()
                .expect("not poisoned")
                .push("enter outer".to_string());
            true
        })
        .on_move(move |_details| {
            move_log
                .lock()
                .expect("not poisoned")
                .push("move outer".to_string());
        })
        .on_accept(move |details| {
            *accepted_for_cb.lock().expect("not poisoned") = Some(details.data);
        })
    };
    let scoped = stack_with(outer, parcel());

    scoped.dispatch_pointer_down(170.0, 170.0);
    scoped.dispatch_pointer_move(50.0, 50.0);

    assert_eq!(
        transitions(&log),
        vec![
            "enter inner".to_string(),
            "enter outer".to_string(),
            "move inner".to_string(),
            "move outer".to_string()
        ],
        "a vetoed inner target stays entered and keeps receiving moves; the \
         search continues outwards to the first target that accepts"
    );

    scoped.dispatch_pointer_up(50.0, 50.0);
    assert_eq!(
        accepted.lock().expect("not poisoned").as_deref(),
        Some("parcel"),
        "the drop lands on the outer target, the one that actually accepted"
    );
}

/// Overlapping siblings: the one painted on top is the one entered, and the
/// one beneath it is not — the hit path is leaf-first in reverse paint order,
/// and the first acceptance stops the search.
#[test]
fn the_topmost_of_two_overlapping_targets_is_the_one_entered() {
    let log = Arc::new(StdMutex::new(Vec::new()));
    let scoped = lay_out(
        Stack::new(vec![
            Positioned::new(logging_target(&log, "beneath"))
                .left(0.0)
                .top(0.0)
                .width(100.0)
                .height(100.0)
                .into_view()
                .boxed(),
            Positioned::new(logging_target(&log, "above"))
                .left(0.0)
                .top(0.0)
                .width(100.0)
                .height(100.0)
                .into_view()
                .boxed(),
            Positioned::new(parcel())
                .left(150.0)
                .top(150.0)
                .width(40.0)
                .height(40.0)
                .into_view()
                .boxed(),
        ])
        .fit(StackFit::Expand),
        tight(200.0, 200.0),
    );

    scoped.dispatch_pointer_down(170.0, 170.0);
    scoped.dispatch_pointer_move(50.0, 50.0);

    assert_eq!(
        transitions(&log),
        vec!["enter above".to_string(), "move above".to_string()],
        "only the topmost overlapping target is entered — the one beneath is \
         shadowed by its acceptance"
    );
}

/// A target under a transform is told where the drag is in its OWN space, not
/// only the root's.
///
/// The oracle gives a target the global position and nothing else; a Dart
/// target that wants a local one calls `globalToLocal` on its render object,
/// which FLUI callback code cannot reach.
///
/// `Transform::scale` scales about the child's CENTRE, so for the 100x100
/// target box the map is `local = centre + (global - centre) / 2`. A drag at
/// global (40, 60) therefore lands at (50, 50) + (-10, 10) / 2 = (45, 55) —
/// a value neither the raw global position nor any origin subtraction can
/// produce, so only composing the entry's real transform gets it right.
#[test]
fn a_transformed_target_is_told_the_drag_position_in_its_own_space() {
    let seen = Arc::new(StdMutex::new(None));
    let seen_for_cb = Arc::clone(&seen);
    let target = DragTarget::<String>::new(|_candidates, _rejected| SizedBox::expand().boxed())
        .on_move(move |details| {
            *seen_for_cb.lock().expect("not poisoned") =
                Some((details.offset, details.local_offset));
        });
    let scoped = stack_with(
        Transform::scale(2.0, 2.0).child(SizedBox::new(50.0, 50.0).child(target)),
        parcel(),
    );

    scoped.dispatch_pointer_down(170.0, 170.0);
    scoped.dispatch_pointer_move(40.0, 60.0);

    let (global, local) = seen
        .lock()
        .expect("not poisoned")
        .expect("the drag must have reached the transformed target");
    assert_eq!(
        global,
        offset(40.0, 60.0),
        "`offset` stays the oracle's global position"
    );
    assert_eq!(
        local,
        offset(45.0, 55.0),
        "`local_offset` must be the same point mapped through the target's \
         own global-to-local transform — see this test's doc for the \
         centre-anchored derivation; neither the global point nor an origin \
         subtraction produces it"
    );
}

/// A target that leaves the tree mid-drag receives nothing more, does not
/// panic, and does not let the drag report a completed drop into a widget
/// that no longer exists.
///
/// The slot outlives the element by `Arc` — the drag is still holding it — so
/// the danger is not a dangling reference but a live one that keeps calling
/// into a disposed state.
#[test]
fn a_target_removed_mid_drag_receives_nothing_further_and_accepts_nothing() {
    let log = Arc::new(StdMutex::new(Vec::new()));
    let accepted_flag = Arc::new(StdMutex::new(None));
    let accepted_for_cb = Arc::clone(&accepted_flag);
    let draggable = parcel().on_drag_end(move |details| {
        *accepted_for_cb.lock().expect("not poisoned") = Some(details.was_accepted);
    });
    let mut scoped = stack_with(logging_target(&log, "inbox"), draggable.clone());

    scoped.dispatch_pointer_down(170.0, 170.0);
    scoped.dispatch_pointer_move(50.0, 50.0);
    assert_eq!(
        transitions(&log),
        vec!["enter inbox".to_string(), "move inbox".to_string()],
        "premise: the drag is inside the target before it is removed"
    );

    // Same tree shape, same slots — only the target's content is swapped for
    // a plain box, so the `Draggable`'s own element (and its in-flight drag)
    // survives while the `DragTarget` element is disposed.
    log.lock().expect("not poisoned").clear();
    scoped.pump_widget(stack_root(SizedBox::expand(), draggable));

    scoped.dispatch_pointer_move(45.0, 45.0);
    scoped.dispatch_pointer_up(45.0, 45.0);

    assert_eq!(
        transitions(&log),
        Vec::<String>::new(),
        "a target whose element is gone must receive no further move, leave, \
         or accept"
    );
    assert_eq!(
        *accepted_flag.lock().expect("not poisoned"),
        Some(false),
        "a drop onto a target that has left the tree was not accepted by \
         anything, and must not be reported as completed"
    );
}

/// A drop on a retired slot still clears the pointer's entry.
///
/// The retired slot answers `false` — the target left the tree and did not
/// receive the data (see `DragTargetSlot::did_drop`'s doc) — but "nothing
/// happened" must not mean "nothing was cleaned up". Returning before the
/// removal would strand the pointer's candidate entry, and with it a strong
/// reference to the drag's payload, for as long as anything holds the slot.
#[test]
fn a_drop_on_a_retired_slot_clears_the_entry_even_though_it_accepts_nothing() {
    let accepts = Arc::new(AtomicUsize::new(0));
    let accepts_for_cb = Arc::clone(&accepts);
    let target = string_target().on_accept(move |_details| {
        accepts_for_cb.fetch_add(1, Ordering::SeqCst);
    });
    let mut state = target.create_state();
    let slot = state.slot();
    let p = pointer(1);

    assert!(slot.did_enter(p, &erase("a".to_string()), at_origin()));
    assert_eq!(
        state.candidate_data().len(),
        1,
        "premise: the pointer is a candidate before the target is disposed"
    );

    // Exactly what the element's own disposal does.
    ViewState::<DragTarget<String>>::dispose(&mut state);

    assert!(
        !slot.did_drop(p, at_origin()),
        "a target that left the tree accepts nothing"
    );
    assert_eq!(
        accepts.load(Ordering::SeqCst),
        0,
        "...and its on_accept must not fire"
    );
    assert!(
        state.candidate_data().is_empty(),
        "but the pointer's entry must still be gone — a retired slot that \
         keeps it strands the standing and the drag payload it holds"
    );
}

/// A `Draggable` and a `DragTarget` inside a `Navigator`'s route, so
/// `Overlay::maybe_of` finds the overlay a real app gives it and `feedback`
/// genuinely mounts.
///
/// The draggable sits at the top-left corner and the target fills
/// (60, 60)-(160, 160), so a drag from (20, 20) to (100, 100) both lands
/// inside the target and leaves the feedback — painted at the tracked
/// displacement (80, 80) — squarely on top of the probe position.
fn lay_out_drag_over_target_with_overlay(
    target: DragTarget<String>,
    draggable: Draggable<String>,
) -> LaidOut {
    let handle = NavigatorHandle::new();
    handle.seed_initial(SimpleRoute::<i32>::new(move |_ctx| {
        Stack::new(vec![
            Positioned::new(target.clone())
                .left(60.0)
                .top(60.0)
                .width(100.0)
                .height(100.0)
                .into_view()
                .boxed(),
            Positioned::new(draggable.clone())
                .left(0.0)
                .top(0.0)
                .width(40.0)
                .height(40.0)
                .into_view()
                .boxed(),
        ])
        .fit(StackFit::Expand)
        .boxed()
    }));
    lay_out(Navigator::new(handle), tight(200.0, 200.0))
}

/// Feedback painted under the pointer must not hide the target from the
/// drag's own hit test.
///
/// The feedback layer is the topmost overlay entry and sits under the pointer
/// by construction, so an *opaque* feedback widget claims the probe position
/// and stops the walk before it reaches any `DragTargetSlot` beneath — the
/// drag would leave the target it is visibly hovering. Flutter's default is
/// `ignoringFeedbackPointer: true` for exactly this reason.
///
/// The feedback here is deliberately hit-testable (`Listener` with
/// `HitTestBehavior::Opaque`): a bare `SizedBox`, which the other feedback
/// cases use, claims no hits at all and so could never demonstrate the
/// blocking either way.
#[test]
fn opaque_feedback_under_the_pointer_does_not_hide_the_target_beneath_it() {
    let log = Arc::new(StdMutex::new(Vec::new()));
    let draggable = parcel().feedback(|| {
        Listener::new()
            .behavior(HitTestBehavior::Opaque)
            .child(SizedBox::new(60.0, 60.0))
            .boxed()
    });
    let mut scoped =
        lay_out_drag_over_target_with_overlay(logging_target(&log, "inbox"), draggable);

    scoped.dispatch_pointer_down(20.0, 20.0);
    settle_one_frame(&mut scoped); // the feedback entry mounts, at displacement zero

    scoped.dispatch_pointer_move(100.0, 100.0);
    assert_eq!(
        transitions(&log),
        vec!["enter inbox".to_string(), "move inbox".to_string()],
        "premise: the drag reaches the target on the move that precedes the \
         feedback repositioning onto the probe point"
    );
    settle_one_frame(&mut scoped); // the feedback anchor rebuilds at (80, 80)

    log.lock().expect("not poisoned").clear();
    scoped.dispatch_pointer_move(101.0, 101.0);

    assert_eq!(
        transitions(&log),
        vec!["move inbox".to_string()],
        "the drag must still see the target under its own feedback — a leave \
         here means the feedback claimed the probe position and hid it"
    );
}

/// A target that records the payload each drag offers it, tagged with `name`.
fn payload_logging_target(
    log: &Arc<StdMutex<Vec<String>>>,
    name: &'static str,
) -> DragTarget<String> {
    let enter_log = Arc::clone(log);
    DragTarget::<String>::new(|_candidates, _rejected| SizedBox::expand().boxed()).on_will_accept(
        move |details| {
            enter_log
                .lock()
                .expect("not poisoned")
                .push(format!("{name} saw {}", details.data));
            true
        },
    )
}

/// Two targets side by side and a draggable clear of both, in a shape stable
/// enough to swap through `pump_widget` while a drag is in flight.
fn two_target_root(
    left: DragTarget<String>,
    right: DragTarget<String>,
    draggable: Draggable<String>,
) -> Stack {
    Stack::new(vec![
        Positioned::new(left)
            .left(0.0)
            .top(0.0)
            .width(80.0)
            .height(80.0)
            .into_view()
            .boxed(),
        Positioned::new(right)
            .left(100.0)
            .top(0.0)
            .width(80.0)
            .height(80.0)
            .into_view()
            .boxed(),
        Positioned::new(draggable)
            .left(0.0)
            .top(150.0)
            .width(40.0)
            .height(40.0)
            .into_view()
            .boxed(),
    ])
    .fit(StackFit::Expand)
}

/// A drag carries the payload it started with, even if the widget's `data`
/// changes underneath it.
///
/// The oracle captures `widget.data` when it builds the `_DragAvatar` and
/// hands that same value to every target for the drag's whole life. Reading it
/// live instead splits one drag in two: targets entered before the rebuild
/// hold the original value (the slot stored it at `did_enter`), targets
/// entered after get the replacement — so crossing a boundary silently changes
/// what is being dragged.
#[test]
fn a_drag_delivers_the_payload_it_started_with_after_the_widget_rebuilds() {
    let log = Arc::new(StdMutex::new(Vec::new()));
    let mut scoped = lay_out(
        two_target_root(
            payload_logging_target(&log, "left"),
            payload_logging_target(&log, "right"),
            Draggable::<String>::new(child()).data("first".to_string()),
        ),
        tight(200.0, 200.0),
    );

    scoped.dispatch_pointer_down(20.0, 170.0);
    scoped.dispatch_pointer_move(40.0, 40.0);
    assert_eq!(
        transitions(&log),
        vec!["left saw first".to_string()],
        "premise: the drag enters the left target carrying its original payload"
    );

    // A parent rebuild replaces the draggable's data mid-drag. The element
    // (and its in-flight drag) survives: same tree shape, same position.
    scoped.pump_widget(two_target_root(
        payload_logging_target(&log, "left"),
        payload_logging_target(&log, "right"),
        Draggable::<String>::new(child()).data("second".to_string()),
    ));

    log.lock().expect("not poisoned").clear();
    scoped.dispatch_pointer_move(140.0, 40.0);

    assert_eq!(
        transitions(&log),
        vec!["right saw first".to_string()],
        "the second target must be offered the SAME payload the first one was \
         — a drag that changes what it is carrying halfway across the screen \
         is one drag delivering two different values"
    );
}

/// The probe follows the feedback the drag actually mounted, not a
/// `feedback_offset` a later rebuild introduced.
///
/// `discover` displaces the hit test by `feedback_offset` so it lands on the
/// feedback rather than the bare pointer (the oracle's `globalPosition +
/// feedbackOffset`). The mounted `FeedbackAnchor` keeps the offset captured
/// when its entry was created, so reading the value live instead would aim the
/// probe somewhere the feedback is not — the two have to be one snapshot.
#[test]
fn the_probe_uses_the_feedback_offset_the_drag_started_with() {
    let log = Arc::new(StdMutex::new(Vec::new()));
    let mut scoped = lay_out(
        stack_root(logging_target(&log, "inbox"), parcel()),
        tight(200.0, 200.0),
    );

    scoped.dispatch_pointer_down(170.0, 170.0);
    scoped.dispatch_pointer_move(50.0, 50.0);
    assert_eq!(
        transitions(&log),
        vec!["enter inbox".to_string(), "move inbox".to_string()],
        "premise: the drag is inside the target before the rebuild"
    );

    // A rebuild introduces a feedback offset far larger than the viewport.
    scoped.pump_widget(stack_root(
        logging_target(&log, "inbox"),
        parcel().feedback_offset(offset(500.0, 500.0)),
    ));

    log.lock().expect("not poisoned").clear();
    scoped.dispatch_pointer_move(51.0, 51.0);

    assert_eq!(
        transitions(&log),
        vec!["move inbox".to_string()],
        "the in-flight drag must keep probing where its own feedback is — \
         adopting the new offset aims the hit test 500px off the target it is \
         sitting on and leaves it"
    );
}
