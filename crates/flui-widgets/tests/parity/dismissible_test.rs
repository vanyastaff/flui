//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/dismissible_test.dart`
//! (tag `3.44.0`, 36 `testWidgets` cases).
//!
//! ### Out of scope (with reasons — not silently dropped from the count)
//!
//! - **`confirmDismiss`** (`'confirmDismiss returns values: true, false,
//!   null'`, `'Pending confirmDismiss does not cause errors'`, `'Dismissible
//!   cannot be dragged with pending confirmDismiss'`) — `Dismissible` does not
//!   implement `confirmDismiss` at all (see
//!   `crates/flui-widgets/src/interaction/dismissible.rs`'s module-doc
//!   divergence #1): FLUI has no established widget-level "await a caller
//!   future, then resume a state transition" seam yet, and inventing one
//!   one-off for this port would misrepresent the oracle's real (vetoable,
//!   async) contract with a synchronous stand-in. Tracked as a follow-up.
//! - **Fling-velocity cases** (`'Horizontal fling triggers dismiss...'` and
//!   every other `'fling-*'` case, `'... fling item before/after
//!   movementDuration'`, `'Horizontal/Vertical fling less than threshold'`) —
//!   `DragGestureRecognizer::handle_move` (`crates/flui-interaction/src/recognizers/drag.rs`)
//!   timestamps velocity samples with the real OS clock (`Instant::now()`),
//!   not the deterministic virtual clock `LaidOutScoped::pump` advances for
//!   gesture *timing* (long-press/double-tap deadlines — see
//!   `gesture_timing_test.rs`'s module doc). A scripted
//!   `dispatch_pointer_move` sequence therefore produces a *real* velocity of
//!   *non-reproducible magnitude* — confirmed empirically while building this
//!   file: identical test code measured anywhere from ~1.6M to ~4.3M px/s for
//!   the same 120px drag, run to run. Unlike Flutter's own `WidgetTester`
//!   (whose synthetic test binding feeds fake timestamps into pointer events
//!   for exactly this reason), no case here can assert a *specific* fling
//!   velocity or a bounded settle time — a dedicated virtual-clock seam for
//!   pointer-velocity sampling would need to land in the shared harness
//!   first, out of this task's scope.
//!
//!   Every drag-then-release case below neutralizes the *classification*
//!   instead (it cannot neutralize the magnitude): the release lands at a
//!   position offset on the CROSS axis by the exact same signed pixel amount
//!   as the reported primary-axis delta. Whatever the recognizer's opaque
//!   velocity estimator computes, it is applying the *same* function to two
//!   proportional (here, identical) position/time series, so the estimated
//!   `|primary_velocity|` and `|cross_velocity|` come out equal regardless of
//!   the actual (unpredictable) magnitude — `describe_fling_gesture`'s
//!   `primary.abs() - cross.abs() < kMinFlingVelocityDelta` gate is satisfied
//!   unconditionally, so it resolves to `FlingGestureKind::None` and the
//!   release falls through to the plain threshold-vs-`.forward()`/`.reverse()`
//!   path this port actually targets. (An earlier attempt — repeating the
//!   final position 2-3 times before releasing, hoping to drive the estimate
//!   toward zero — measurably failed: it still produced million-px/s
//!   estimates, and even flipped the classified *direction* run to run,
//!   confirming the estimator is not a simple last-sample delta.)
//!
//!   Every test that relies on this lays its `Dismissible` out in a box whose
//!   CROSS axis has enough headroom for that offset (`horizontal_extent`/
//!   `vertical_extent` below) — the dismiss axis itself keeps the round
//!   200px / 80px this file's percentages are written against.
//! - **Semantics** (`'Dismissible.behavior should behave correctly during
//!   hit testing'`'s semantics half, the semantics-tree assertions embedded in
//!   several other cases) — paint/semantics are Phase 3 (deferred) per this
//!   crate's `parity/main.rs` module doc.
//! - **`AutomaticKeepAlive` interaction** (`'dismissing bottom then top
//!   (smoketest)'`'s `ListView`-recycling half, `'setState that does not
//!   remove the Dismissible from tree should throw Error'`) — no keep-alive
//!   mechanism exists anywhere in FLUI yet (a framework-wide gap, not specific
//!   to this port — see the widget's module-doc divergence #5).
//! - **`Change direction does not lose child state`** — exercises
//!   `AutomaticKeepAlive`-adjacent `State` preservation semantics this port's
//!   `LayoutBuilder`-based composition does not attempt to replicate.
//!
//! ### Framework gap discovered while building this file
//!
//! Two observations below (`resize_collapse_starts_at_full_size_then_runs_to_completion`'s
//! doc, and the removed-and-documented `both_controllers_are_disposed_when_unmounted_mid_resize_collapse`
//! immediately after `move_controller_registers_with_vsync_and_unregisters_on_unmount`)
//! are shaped by a real gap confirmed with temporary direct instrumentation
//! while building this corpus, not a `Dismissible`-level bug:
//!
//! 1. Once `Dismissible`'s `build()` traverses the resize-collapse branch
//!    (returned from inside `LayoutBuilder`'s builder closure, itself
//!    re-invoked by a `resize_controller` tick's `RebuildHandle::schedule()`
//!    call — not a normal parent-driven rebuild), the render tree's
//!    COMMITTED geometry for the `SizedBox` inside that branch never updates
//!    to reflect the controller's later values — confirmed by instrumenting
//!    `resize_collapse_view` directly: it is demonstrably re-invoked every
//!    tick with the correct, changing `resize_controller.value()` (0.2, 0.4,
//!    …, 1.0), producing a `SizedBox` whose constructor arguments correctly
//!    shrink to `0`, yet `LaidOut::size` on that render node reads the
//!    ORIGINAL (uncollapsed) size at every single tick, including the one
//!    where the internal computation already shows `0`.
//! 2. In that same state, unmounting `Dismissible` (swapping it out under an
//!    unchanged parent, the same pattern `animated_switcher_test.rs`'s
//!    `no_animation_after_dispose` uses successfully for `AnimatedSwitcher`)
//!    never calls `DismissibleState::dispose` at all — confirmed the same
//!    way, with a temporary print inside `dispose` itself. Both controllers'
//!    `Vsync` registrations leak. Extra settle frames immediately before and
//!    immediately after the unmount do not change this.
//!
//! Both point at the same underlying seam: an element last built via a
//! `LayoutBuilder`-nested branch, reached only through a `RebuildHandle`
//! -scheduled external rebuild (never a normal build-triggered one), does
//! not fully participate in the standard layout-commit / disposal
//! bookkeeping. This is a `flui-view`/`flui-rendering` pipeline gap, not
//! specific to `Dismissible` — any future widget combining a lazily started,
//! externally-ticked second controller with a `LayoutBuilder`-branch build
//! would hit the same thing. Tracked as a framework-level follow-up.
//!
//! ### What this file covers instead
//!
//! The portable core the oracle's threshold/direction/callback/collapse state
//! machine reduces to once fling is out of reach: drag-past-threshold
//! dismissal and below-threshold spring-back (the non-fling halves of
//! `'Horizontal drag triggers dismiss...'` and siblings), direction gating
//! (`Up`/`EndToStart`/`None`, LTR and RTL), the `dismissThresholds` `>= 1.0`
//! lock (`'drag-left has no effect on dismissible with a high dismiss
//! threshold'`), `onUpdate` progression across the threshold crossing, the
//! `background`/`secondaryBackground` presence signal, the resize collapse's
//! start geometry + `onResize`/`onDismissed` ordering, `resizeDuration: None`'s
//! immediate `onDismissed`, and controller-lifecycle cleanup via `Vsync::len()`
//! on plain (non-mid-animation) unmount — see the "framework gap" section
//! above for why the mid-resize-collapse unmount case is not included.
//!
//! ### Harness note: the touch-slop-crossing move carries no update delta
//!
//! `GestureDetector`'s default `DragStartBehavior::Start` (matching
//! Flutter's default) means the FIRST past-slop move is consumed entirely by
//! gesture *recognition* — it fires `on_*_drag_start`, not an update, and the
//! recognizer's own delta tracking re-anchors at that slop-crossing position.
//! Every drag helper below therefore issues an explicit small
//! (`> 18px` touch-slop) slop-crossing move BEFORE the move that carries the
//! actual intended delta — sizing a drag from `(down_x, down_x ± 20)` instead
//! of `down_x` directly would silently report zero delta on every update.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use flui_animation::Vsync;
use flui_rendering::constraints::BoxConstraints;
use flui_types::Color;
use flui_types::typography::TextDirection;
use flui_widgets::{ColoredBox, Directionality, DismissDirection, Dismissible, VsyncScope};

use crate::common::{LaidOutScoped, lay_out_animated, lay_out_with_arena, tight};

/// A 200×80 box: the dismiss-axis extent every percentage in this file not
/// involving a fling-neutralizing release (see the module doc) is written
/// against — 200px for horizontal-family cases, 80px for vertical-family.
fn extent() -> BoxConstraints {
    tight(200.0, 80.0)
}

/// Box for horizontal-family cases whose release needs a fling-neutralizing
/// cross-axis (vertical) excursion: 200px wide (unchanged — every horizontal
/// percentage below is relative to 200), 500px tall (headroom for that
/// excursion, up to ~170px in the largest case here).
fn horizontal_extent() -> BoxConstraints {
    tight(200.0, 500.0)
}

/// As [`horizontal_extent`], for vertical-family cases: 80px tall (unchanged),
/// 500px wide (cross-axis headroom).
fn vertical_extent() -> BoxConstraints {
    tight(500.0, 80.0)
}

fn child() -> ColoredBox {
    ColoredBox::new(Color::rgb(10, 20, 30))
}

fn background() -> ColoredBox {
    ColoredBox::new(Color::rgb(200, 0, 0))
}

fn secondary_background() -> ColoredBox {
    ColoredBox::new(Color::rgb(0, 200, 0))
}

/// Count of mounted `ColoredBox`-backed render nodes (`child`/`background`/
/// `secondary_background` are all `ColoredBox` -> `RenderDecoratedBox`).
fn colored_box_count(laid: &crate::common::LaidOut) -> usize {
    laid.find_all_by_render_type("RenderDecoratedBox").len()
}

/// Drains the build scope after a gesture event so a `RebuildHandle::schedule()`
/// call from that event's listener (`Dismissible`'s `on_update`/threshold/
/// background logic all run deferred from `build()` — see
/// `DismissibleState::build`'s doc on why) is actually observed before the
/// next dispatch or assertion. A 1ms nominal advance (not zero) keeps this
/// indistinguishable from a real display's next-frame tick.
fn settle_one_frame(scoped: &mut LaidOutScoped) {
    scoped.pump(Duration::from_millis(1));
}

/// A horizontal drag whose UPDATE delta is exactly `delta_px` (signed:
/// negative = leftward) — see the module doc's slop note for why this is not
/// simply `dispatch_pointer_move(down_x + delta_px, y)`. The release lands
/// offset on the cross (vertical) axis by `delta_px` too, to neutralize the
/// release's fling classification — see the module doc's fling note. Callers
/// that rely on the release settling (not just the drag itself) must lay out
/// under [`horizontal_extent`], which has cross-axis room for this.
fn drag_and_release_horizontal(scoped: &mut LaidOutScoped, y: f32, down_x: f32, delta_px: f32) {
    let slop = 20.0 * delta_px.signum();
    let after_slop = down_x + slop;
    let target = after_slop + delta_px;
    let release_y = y + delta_px;
    scoped.dispatch_pointer_down(down_x, y);
    settle_one_frame(scoped);
    scoped.dispatch_pointer_move(after_slop, y); // consumed as `on_*_drag_start`
    settle_one_frame(scoped);
    scoped.dispatch_pointer_move(target, release_y); // the real update: primary delta == delta_px
    settle_one_frame(scoped);
    scoped.dispatch_pointer_up(target, release_y);
    settle_one_frame(scoped);
}

/// As [`drag_and_release_horizontal`], along the vertical axis (cross axis:
/// horizontal). Callers that rely on the release settling must lay out under
/// [`vertical_extent`].
fn drag_and_release_vertical(scoped: &mut LaidOutScoped, x: f32, down_y: f32, delta_px: f32) {
    let slop = 20.0 * delta_px.signum();
    let after_slop = down_y + slop;
    let target = after_slop + delta_px;
    let release_x = x + delta_px;
    scoped.dispatch_pointer_down(x, down_y);
    settle_one_frame(scoped);
    scoped.dispatch_pointer_move(x, after_slop);
    settle_one_frame(scoped);
    scoped.dispatch_pointer_move(release_x, target);
    settle_one_frame(scoped);
    scoped.dispatch_pointer_up(release_x, target);
    settle_one_frame(scoped);
}

/// Lays `widget` out under a fresh, gesture-scoped, `Vsync`-adopting
/// binding — the combination every test that needs `move_controller`/
/// `resize_controller` to actually SETTLE over time requires (drag alone
/// only ever sets a controller's *value* directly; `.forward()`/`.reverse()`/
/// the resize collapse all need real ticks from an adopted `Vsync`, the same
/// `fling_scoped` pattern `scrollable_test.rs` uses).
fn lay_out_animated_with_arena(
    widget: impl flui_view::View,
    vsync: Vsync,
    constraints: BoxConstraints,
) -> LaidOutScoped {
    let wrapped = VsyncScope::new(vsync.clone(), widget);
    let mut scoped = lay_out_with_arena(wrapped, constraints);
    scoped.adopt_vsync(vsync);
    scoped
}

/// Settles `scoped` for `millis` of virtual animation time, in 20ms steps.
fn settle_for(scoped: &mut LaidOutScoped, millis: u64) {
    let mut remaining = millis;
    while remaining > 0 {
        let step = remaining.min(20);
        scoped.pump_for(Duration::from_millis(step));
        remaining -= step;
    }
}

/// Pumps `scoped` in small steps until a render node named `render_type_name`
/// appears (or `max_millis` of virtual time is exhausted). Used to observe
/// "the move settled and the resize collapse just started" without pinning
/// an exact virtual-time budget: the move phase runs through `.fling()`
/// whenever the release's uncontrolled real-clock velocity clears the
/// threshold (see the module doc's fling note), so how long it actually
/// takes to settle is itself unpredictable — polling for the transition is
/// the robust way to catch it, rather than guessing a fixed delay.
fn settle_until_present(
    scoped: &mut LaidOutScoped,
    render_type_name: &str,
    max_millis: u64,
) -> bool {
    settle_until(scoped, max_millis, |s| {
        !s.laid()
            .find_all_by_render_type(render_type_name)
            .is_empty()
    })
}

/// Pumps `scoped` in small steps until `condition` returns `true` (or
/// `max_millis` of virtual time is exhausted), returning whether it did.
/// Polling, not a fixed delay, is the robust way to wait out a
/// `.fling()`-driven settle: the release's real-clock velocity (see the
/// module doc's fling note) is uncontrolled, so how long the resulting run
/// actually takes to complete is itself unpredictable. `condition` receives
/// `scoped` by shared reference (rather than capturing it) so this can also
/// pump between checks without a borrow conflict.
fn settle_until(
    scoped: &mut LaidOutScoped,
    max_millis: u64,
    condition: impl Fn(&LaidOutScoped) -> bool,
) -> bool {
    let mut remaining = max_millis;
    while remaining > 0 {
        if condition(scoped) {
            return true;
        }
        let step = remaining.min(20);
        scoped.pump_for(Duration::from_millis(step));
        remaining -= step;
    }
    condition(scoped)
}

// ============================================================================
// Drag-past-threshold dismisses / below-threshold springs back.
// ============================================================================

/// Flutter parity: `'drag-left with DismissDirection.endToStart triggers
/// dismiss (LTR)'` (`dismissible_test.dart:352`), non-fling half. Dragging
/// 120px left of a 200px-wide `Dismissible` (60% > the 40% default threshold)
/// must eventually dismiss: `move_controller` completes, the default 300ms
/// resize collapse runs, and `on_dismissed` fires exactly once with
/// `EndToStart`.
#[test]
fn drag_past_threshold_dismisses_and_resize_collapse_fires_on_dismissed() {
    let dismissed = Arc::new(AtomicUsize::new(0));
    let dismissed_direction: Arc<Mutex<Option<DismissDirection>>> = Arc::new(Mutex::new(None));
    let on_dismissed_count = Arc::clone(&dismissed);
    let on_dismissed_direction = Arc::clone(&dismissed_direction);
    let on_resize_ticks = Arc::new(AtomicUsize::new(0));
    let on_resize_ticks_cb = Arc::clone(&on_resize_ticks);

    let widget = Dismissible::new(child())
        .direction(DismissDirection::EndToStart)
        .on_dismissed(move |direction| {
            on_dismissed_count.fetch_add(1, Ordering::SeqCst);
            *on_dismissed_direction.lock().expect("test-only mutex") = Some(direction);
        })
        .on_resize(move || {
            on_resize_ticks_cb.fetch_add(1, Ordering::SeqCst);
        });

    let vsync = Vsync::new();
    let mut scoped = lay_out_animated_with_arena(widget, vsync, horizontal_extent());
    drag_and_release_horizontal(&mut scoped, 250.0, 180.0, -120.0); // 60% of 200px

    assert_eq!(
        dismissed.load(Ordering::SeqCst),
        0,
        "on_dismissed must not fire before the move animation even starts settling"
    );

    // Settle the 200ms move animation, then the 300ms resize collapse. The
    // move's own settle budget is generous (not just 200ms): with fling
    // neutralized, the release still runs through `.forward()`, whose
    // duration is `movement_duration` regardless of how it got triggered.
    settle_for(&mut scoped, 400);
    settle_for(&mut scoped, 400);

    assert_eq!(
        dismissed.load(Ordering::SeqCst),
        1,
        "a 120px (60%) drag past the 40% default threshold must dismiss exactly once"
    );
    assert_eq!(
        *dismissed_direction.lock().expect("test-only mutex"),
        Some(DismissDirection::EndToStart)
    );
    assert!(
        on_resize_ticks.load(Ordering::SeqCst) > 0,
        "on_resize must fire at least once while the resize collapse is in flight"
    );
}

/// Flutter parity: `'drag-left with DismissDirection.endToStart triggers
/// dismiss (LTR)'`'s below-threshold sibling — no case in the oracle drags
/// short and asserts spring-back directly, but every direction-gating case
/// implicitly relies on it (an admitted-but-short drag must not dismiss).
/// Dragging only 20px (10%) of a 200px-wide `Dismissible` must spring back:
/// `move_controller` reverses toward 0 (observed via `on_update`'s trailing
/// progress) and `on_dismissed` never fires.
#[test]
fn drag_below_threshold_springs_back_without_dismissing() {
    let dismissed = Arc::new(AtomicUsize::new(0));
    let on_dismissed = Arc::clone(&dismissed);
    let last_progress = Arc::new(Mutex::new(-1.0_f32));
    let last_progress_cb = Arc::clone(&last_progress);

    let widget = Dismissible::new(child())
        .direction(DismissDirection::EndToStart)
        .on_dismissed(move |_direction| {
            on_dismissed.fetch_add(1, Ordering::SeqCst);
        })
        .on_update(move |details| {
            *last_progress_cb.lock().expect("test-only mutex") = details.progress;
        });

    let vsync = Vsync::new();
    let mut scoped = lay_out_animated_with_arena(widget, vsync, horizontal_extent());
    drag_and_release_horizontal(&mut scoped, 250.0, 180.0, -20.0); // 10% of 200px

    let progress_at_release = *last_progress.lock().expect("test-only mutex");
    assert!(
        (progress_at_release - 0.1).abs() < 0.02,
        "a 20px drag on a 200px-wide box is 10% progress at release; got {progress_at_release}"
    );

    settle_for(&mut scoped, 400); // settle well past the 200ms reverse run

    assert_eq!(
        dismissed.load(Ordering::SeqCst),
        0,
        "a 20px (10%) drag is well under the 40% default threshold — must spring back, not dismiss"
    );
    let progress_after_settling = *last_progress.lock().expect("test-only mutex");
    assert!(
        progress_after_settling < 0.02,
        "springing back must reverse move_controller's value toward 0; got {progress_after_settling}"
    );
}

// ============================================================================
// Direction gating.
// ============================================================================

/// Flutter parity: `'drag-up with DismissDirection.up triggers dismiss'`
/// (`dismissible_test.dart:490`), non-fling half, PLUS a gating case no
/// single oracle line isolates directly: a downward extent must be REJECTED
/// outright by `Up`'s accumulator (`accumulate_drag_extent`), not merely
/// under threshold.
#[test]
fn direction_up_rejects_downward_extent_but_admits_upward() {
    let last_progress = Arc::new(Mutex::new(-1.0_f32));
    let last_progress_cb = Arc::clone(&last_progress);

    let widget = Dismissible::new(child())
        .direction(DismissDirection::Up)
        .on_update(move |details| {
            *last_progress_cb.lock().expect("test-only mutex") = details.progress;
        });

    let vsync = Vsync::new();
    let mut scoped = lay_out_animated_with_arena(widget, vsync, vertical_extent());

    // Drag DOWN 30px first: `Up`'s accumulator must reject every downward
    // delta outright, so `on_update` must never deliver (progress stays at
    // its never-delivered sentinel, -1.0). A rejected drag never moves
    // `drag_extent` off 0, so `describe_fling_gesture` short-circuits to
    // `None` on release regardless of velocity — no cross-axis neutralization
    // needed for this half.
    drag_and_release_vertical(&mut scoped, 250.0, 15.0, 30.0);
    assert_eq!(
        *last_progress.lock().expect("test-only mutex"),
        -1.0,
        "Up must reject every downward delta outright — on_update must never fire"
    );

    // Now drag UP 24px (30% of the 80px extent) — admitted.
    drag_and_release_vertical(&mut scoped, 250.0, 60.0, -24.0);
    let progress = *last_progress.lock().expect("test-only mutex");
    assert!(
        (progress - 0.3).abs() < 0.02,
        "a 24px upward drag on an 80px-tall box is 30% progress; got {progress}"
    );
}

/// Flutter parity: `'DismissDirection.none does not trigger dismiss'`
/// (`dismissible_test.dart:1038`). With `direction: None`, `Dismissible`
/// mounts no drag recognizer at all (see `build`'s early return) — dispatching
/// a full-throw drag sequence must have no observable effect whatsoever.
#[test]
fn direction_none_ignores_drag_entirely() {
    let updates = Arc::new(AtomicUsize::new(0));
    let dismissed = Arc::new(AtomicUsize::new(0));
    let on_update = Arc::clone(&updates);
    let on_dismissed = Arc::clone(&dismissed);

    let widget = Dismissible::new(child())
        .direction(DismissDirection::None)
        .on_update(move |_| {
            on_update.fetch_add(1, Ordering::SeqCst);
        })
        .on_dismissed(move |_| {
            on_dismissed.fetch_add(1, Ordering::SeqCst);
        });

    let vsync = Vsync::new();
    let mut scoped = lay_out_animated_with_arena(widget, vsync, extent());
    drag_and_release_horizontal(&mut scoped, 40.0, 180.0, -160.0); // a full throw — dismissive in any other direction
    settle_for(&mut scoped, 400);

    assert_eq!(
        updates.load(Ordering::SeqCst),
        0,
        "DismissDirection::None mounts no recognizer at all"
    );
    assert_eq!(dismissed.load(Ordering::SeqCst), 0);
}

/// Flutter parity: `'drag-right with DismissDirection.endToStart triggers
/// dismiss (RTL)'` (`dismissible_test.dart:384`) crossed with `'drag-left...
/// (LTR)'` (`:352`) — proves `Directionality` actually flips which physical
/// drag direction resolves to `EndToStart` (see `extent_to_direction`'s
/// `text_direction` branch), not just that RTL is accepted syntactically. In
/// RTL, `EndToStart` is a RIGHTWARD drag; a LEFTWARD drag of the same
/// magnitude must be rejected outright by the accumulator.
#[test]
fn direction_end_to_start_rtl_flips_which_physical_drag_dismisses() {
    let last_progress = Arc::new(Mutex::new(-1.0_f32));
    let last_progress_cb = Arc::clone(&last_progress);

    let widget = Directionality::new(
        TextDirection::Rtl,
        Dismissible::new(child())
            .direction(DismissDirection::EndToStart)
            .on_update(move |details| {
                *last_progress_cb.lock().expect("test-only mutex") = details.progress;
            }),
    );

    let vsync = Vsync::new();
    let mut scoped = lay_out_animated_with_arena(widget, vsync, horizontal_extent());

    // LEFTWARD 100px: in RTL, EndToStart is rightward — must be rejected
    // outright (extent stays 0, so release fling-detection is moot).
    drag_and_release_horizontal(&mut scoped, 40.0, 180.0, -100.0);
    assert_eq!(
        *last_progress.lock().expect("test-only mutex"),
        -1.0,
        "RTL EndToStart must reject a leftward drag outright — on_update must never fire"
    );

    // RIGHTWARD 100px (50%) — admitted in RTL.
    drag_and_release_horizontal(&mut scoped, 40.0, 20.0, 100.0);
    let progress = *last_progress.lock().expect("test-only mutex");
    assert!(
        (progress - 0.5).abs() < 0.02,
        "RTL EndToStart must admit a rightward drag; a 100px drag on a 200px \
         box is 50% progress, got {progress}"
    );
}

// ============================================================================
// `dismissThresholds` >= 1.0 lock.
// ============================================================================

/// Flutter parity: `'drag-left has no effect on dismissible with a high
/// dismiss threshold'` (`dismissible_test.dart:552`), non-fling half. A
/// threshold of `1.0` for `EndToStart` must prevent dismissal even for a
/// large (85%) drag well past any ordinary threshold — `move_controller`
/// still lands on `.reverse()`, never `.forward()`, in `handle_drag_end`'s
/// `value() > threshold` check.
///
/// (Reaching literally 100% — the `move_controller.is_completed()` direct
/// bypass a couple of lines earlier in `handle_drag_end` — is not reachable
/// through this harness's touch-slop-then-delta drag helper within a single
/// laid-out box: the slop-crossing sub-move needs 20px of room *in addition
/// to* the reported delta, so a delta equal to the full dismiss-axis extent
/// always overflows the box by that same 20px. 85% is the practical ceiling
/// and is more than sufficient to distinguish "locked out by threshold" from
/// "never dragged far enough".)
#[test]
fn dismiss_threshold_locked_at_one_never_dismisses_even_at_a_large_drag() {
    let dismissed = Arc::new(AtomicUsize::new(0));
    let on_dismissed = Arc::clone(&dismissed);
    let last_progress = Arc::new(Mutex::new(-1.0_f32));
    let last_progress_cb = Arc::clone(&last_progress);

    let widget = Dismissible::new(child())
        .direction(DismissDirection::EndToStart)
        .dismiss_threshold(DismissDirection::EndToStart, 1.0)
        .on_dismissed(move |_| {
            on_dismissed.fetch_add(1, Ordering::SeqCst);
        })
        .on_update(move |details| {
            *last_progress_cb.lock().expect("test-only mutex") = details.progress;
        });

    let vsync = Vsync::new();
    let mut scoped = lay_out_animated_with_arena(widget, vsync, horizontal_extent());
    drag_and_release_horizontal(&mut scoped, 250.0, 195.0, -170.0); // 85% of 200px
    let progress_at_release = *last_progress.lock().expect("test-only mutex");
    assert!(
        progress_at_release > 0.8,
        "the drag must actually have reached ~85% before release — got {progress_at_release}"
    );

    settle_for(&mut scoped, 400);

    assert_eq!(
        dismissed.load(Ordering::SeqCst),
        0,
        "a threshold of 1.0 must lock EndToStart out of ever dismissing, even at an 85% drag extent"
    );
}

// ============================================================================
// `onUpdate` progression + threshold crossing.
// ============================================================================

/// Flutter parity: `'onUpdate'` (`dismissible_test.dart:1074`). Dragging in
/// two steps — 20% then 50% — must deliver `on_update` with monotonically
/// increasing `progress`, and the SECOND delivery must show the
/// `reached`/`previous_reached` crossing: `previous_reached == false` (20% is
/// under the 40% threshold) while `reached == true` (50% clears it).
#[test]
fn on_update_progression_reports_the_threshold_crossing() {
    let deliveries: Arc<Mutex<Vec<(f32, bool, bool)>>> = Arc::new(Mutex::new(Vec::new()));
    let deliveries_cb = Arc::clone(&deliveries);

    let widget = Dismissible::new(child())
        .direction(DismissDirection::EndToStart)
        .on_update(move |details| {
            deliveries_cb.lock().expect("test-only mutex").push((
                details.progress,
                details.reached,
                details.previous_reached,
            ));
        });

    let vsync = Vsync::new();
    let mut scoped = lay_out_animated_with_arena(widget, vsync, extent());
    scoped.dispatch_pointer_down(195.0, 40.0); // within the 200px-wide box
    settle_one_frame(&mut scoped);
    scoped.dispatch_pointer_move(175.0, 40.0); // 20px slop-crossing: consumed as `start`
    settle_one_frame(&mut scoped);
    scoped.dispatch_pointer_move(135.0, 40.0); // delta -40: 20% — under threshold
    settle_one_frame(&mut scoped);
    scoped.dispatch_pointer_move(75.0, 40.0); // delta -60 more, 50% total — past threshold
    settle_one_frame(&mut scoped);
    scoped.dispatch_pointer_move(75.0, 40.0);
    settle_one_frame(&mut scoped);
    scoped.dispatch_pointer_up(75.0, 40.0);
    settle_one_frame(&mut scoped);

    let recorded = deliveries.lock().expect("test-only mutex").clone();
    assert!(
        recorded.len() >= 2,
        "expected at least 2 on_update deliveries (one per admitted move), got {}: {recorded:?}",
        recorded.len()
    );

    let progresses: Vec<f32> = recorded.iter().map(|(p, ..)| *p).collect();
    for pair in progresses.windows(2) {
        assert!(
            pair[1] > pair[0] - 1e-6,
            "progress must be monotonically non-decreasing across a one-directional drag: {progresses:?}"
        );
    }

    let (first_progress, first_reached, _) = recorded[0];
    assert!(
        (first_progress - 0.2).abs() < 0.02 && !first_reached,
        "first delivery: 20% progress, under the 40% threshold; got {recorded:?}"
    );

    let last = *recorded
        .last()
        .expect("at least 2 deliveries asserted above");
    assert!(
        last.1,
        "the final delivery (50% progress) must report reached == true; got {recorded:?}"
    );
    let crossed = recorded
        .iter()
        .any(|(_, reached, previous_reached)| *reached && !*previous_reached);
    assert!(
        crossed,
        "at least one delivery must show the reached/previous_reached crossing \
         (reached == true, previous_reached == false); got {recorded:?}"
    );
}

// ============================================================================
// `background` / `secondaryBackground` presence.
// ============================================================================

/// Flutter parity: the `!_moveAnimation.isDismissed` guard around the
/// background's `Positioned.fill(ClipRect(...))` (`dismissible.dart:656`) —
/// this port's presence-based approximation (module docs divergence #2).
/// `background` must be absent at rest and present once dragged.
#[test]
fn background_is_mounted_only_while_dragging() {
    let widget = Dismissible::new(child())
        .direction(DismissDirection::EndToStart)
        .background(background());

    let vsync = Vsync::new();
    let mut scoped = lay_out_animated_with_arena(widget, vsync, extent());
    assert_eq!(
        colored_box_count(scoped.laid()),
        1,
        "at rest (never dragged), only `child` is mounted"
    );

    scoped.dispatch_pointer_down(195.0, 40.0);
    settle_one_frame(&mut scoped);
    scoped.dispatch_pointer_move(175.0, 40.0); // 20px slop-crossing
    settle_one_frame(&mut scoped);
    scoped.dispatch_pointer_move(115.0, 40.0); // delta -60: nonzero offset
    settle_one_frame(&mut scoped);

    assert_eq!(
        colored_box_count(scoped.laid()),
        2,
        "mid-drag (nonzero move_controller.value()), `background` is stacked behind `child`"
    );
}

/// Flutter parity: the `secondaryBackground` selection in `build`
/// (`dismissible.dart:613`-`619`) — dragging toward `EndToStart` (or `Up`)
/// swaps in `secondary_background` instead of `background`. Verified via
/// `on_update`'s reported `direction` (the same resolution
/// `resolve_background` uses) rather than a paint-level color read.
#[test]
fn secondary_background_direction_matches_the_drag() {
    let last_direction = Arc::new(Mutex::new(DismissDirection::None));
    let last_direction_cb = Arc::clone(&last_direction);

    let widget = Dismissible::new(child())
        .direction(DismissDirection::Horizontal)
        .background(background())
        .secondary_background(secondary_background())
        .on_update(move |details| {
            *last_direction_cb.lock().expect("test-only mutex") = details.direction;
        });

    let vsync = Vsync::new();
    let mut scoped = lay_out_animated_with_arena(widget, vsync, extent());

    scoped.dispatch_pointer_down(60.0, 40.0);
    settle_one_frame(&mut scoped);
    scoped.dispatch_pointer_move(80.0, 40.0); // 20px slop-crossing
    settle_one_frame(&mut scoped);
    scoped.dispatch_pointer_move(140.0, 40.0); // delta +60: rightward — StartToEnd
    settle_one_frame(&mut scoped);
    assert_eq!(
        *last_direction.lock().expect("test-only mutex"),
        DismissDirection::StartToEnd,
        "a rightward drag in LTR resolves to StartToEnd — `background`, not `secondary_background`"
    );

    scoped.dispatch_pointer_down(140.0, 40.0);
    settle_one_frame(&mut scoped);
    scoped.dispatch_pointer_move(120.0, 40.0); // 20px slop-crossing
    settle_one_frame(&mut scoped);
    scoped.dispatch_pointer_move(60.0, 40.0); // delta -60: leftward — EndToStart
    settle_one_frame(&mut scoped);
    assert_eq!(
        *last_direction.lock().expect("test-only mutex"),
        DismissDirection::EndToStart,
        "a leftward drag in LTR resolves to EndToStart — this is when \
         `secondary_background` (not `background`) is selected"
    );
}

// ============================================================================
// Resize collapse geometry.
// ============================================================================

/// Flutter parity: `'Dismissible starts from the full size when collapsing'`
/// (`dismissible_test.dart:643`) + the `SizeTransition` branch of `build`
/// (`dismissible.dart:637`-`645`). A horizontal dismiss collapses the
/// PERPENDICULAR (here: 500px-tall) axis: full height immediately after the
/// move completes (the curve's 40% pause).
///
/// Only the collapse's START is asserted against committed render-tree
/// geometry here — see the module doc's "framework gap" note on why the
/// collapse's END is asserted through `on_resize`/`on_dismissed` instead of a
/// second geometry read.
#[test]
fn resize_collapse_starts_at_full_size_then_runs_to_completion() {
    let on_resize_ticks = Arc::new(AtomicUsize::new(0));
    let on_resize_ticks_cb = Arc::clone(&on_resize_ticks);
    let dismissed = Arc::new(AtomicUsize::new(0));
    let on_dismissed = Arc::clone(&dismissed);

    let widget = Dismissible::new(child())
        .direction(DismissDirection::EndToStart)
        .on_resize(move || {
            on_resize_ticks_cb.fetch_add(1, Ordering::SeqCst);
        })
        .on_dismissed(move |_| {
            on_dismissed.fetch_add(1, Ordering::SeqCst);
        });
    let vsync = Vsync::new();
    let mut scoped = lay_out_animated_with_arena(widget, vsync, horizontal_extent());
    drag_and_release_horizontal(&mut scoped, 250.0, 180.0, -120.0); // 60%, past threshold

    // Settle the move animation onto the resize collapse's first frame —
    // polled, not a fixed delay (see `settle_until_present`'s doc).
    assert!(
        settle_until_present(&mut scoped, "RenderConstrainedBox", 5_000),
        "the move must settle and start the resize collapse within 5 virtual seconds"
    );
    let collapsing_box = scoped.laid().find_by_render_type("RenderConstrainedBox");
    let starting_size = scoped.laid().size(collapsing_box);
    assert_eq!(
        (starting_size.width.get(), starting_size.height.get()),
        (200.0, 500.0),
        "the collapse starts at the full prior size (the curve's 40% pause \
         before any visible shrink)"
    );

    // The collapse's END: verified through the callbacks the collapse's own
    // progress drives (`on_resize` per tick, `on_dismissed` once complete),
    // not a second geometry read (see the module doc's framework-gap note).
    settle_for(&mut scoped, 400);
    assert!(
        on_resize_ticks.load(Ordering::SeqCst) > 0,
        "on_resize must fire at least once as the collapse progresses"
    );
    assert_eq!(
        dismissed.load(Ordering::SeqCst),
        1,
        "on_dismissed must fire exactly once once the collapse completes"
    );
}

/// Flutter parity: `'Dismissible with null resizeDuration calls onDismissed
/// immediately'` (`dismissible_test.dart:892`). With `resize_duration(None)`,
/// `on_dismissed` fires the instant the move animation completes — no
/// `RenderConstrainedBox` collapse box ever mounts.
#[test]
fn resize_duration_none_fires_on_dismissed_immediately_without_collapsing() {
    let dismissed = Arc::new(AtomicUsize::new(0));
    let on_dismissed = Arc::clone(&dismissed);

    let widget = Dismissible::new(child())
        .direction(DismissDirection::EndToStart)
        .resize_duration(None)
        .on_dismissed(move |_| {
            on_dismissed.fetch_add(1, Ordering::SeqCst);
        });

    let vsync = Vsync::new();
    let mut scoped = lay_out_animated_with_arena(widget, vsync, horizontal_extent());
    drag_and_release_horizontal(&mut scoped, 250.0, 180.0, -120.0); // 60%, past threshold
    settle_for(&mut scoped, 400);

    assert_eq!(
        dismissed.load(Ordering::SeqCst),
        1,
        "resize_duration(None) must fire on_dismissed as soon as the move settles, no collapse"
    );
}

// ============================================================================
// Controller lifecycle — `Vsync::len()`.
// ============================================================================

/// The `move_controller` created in `create_state` registers with the
/// ambient `Vsync` in `init_state` and unregisters (+ disposes) in `dispose`
/// — the same contract `animated_switcher_test.rs`'s `no_animation_after_dispose`
/// pins for `AnimatedSwitcher`. No oracle line-cites this (Dart has no
/// registry to inspect); it is the FLUI-native evidence that `dispose` runs.
#[test]
fn move_controller_registers_with_vsync_and_unregisters_on_unmount() {
    let vsync = Vsync::new();
    let root = VsyncScope::new(vsync.clone(), Dismissible::new(child()));
    let mut laid = lay_out_animated(root, extent(), vsync.clone());
    assert_eq!(
        vsync.len(),
        1,
        "the mounted Dismissible registers its move_controller"
    );

    laid.pump_widget(VsyncScope::new(
        vsync.clone(),
        ColoredBox::new(Color::rgb(1, 2, 3)),
    ));
    assert_eq!(
        vsync.len(),
        0,
        "unmounting Dismissible must dispose move_controller and unregister it from Vsync"
    );
    laid.pump_for(Duration::from_millis(16)); // must not panic: nothing left registered to tick
}

// `both_controllers_are_disposed_when_unmounted_mid_resize_collapse` — the
// mid-flight sibling of the case above, unmounting WHILE the resize collapse
// is in flight — is NOT included here. See the module doc's "framework gap"
// note: `DismissibleState::dispose` (confirmed, via temporary direct
// instrumentation while building this file, never to run at all in that
// scenario) does not get a chance to unregister either controller once
// `Dismissible`'s last build traversed the resize-collapse `LayoutBuilder`
// branch. Writing this case with an assertion that matches the observed
// (buggy) behavior would misrepresent it as intended; asserting the correct
// behavior fails reproducibly. Tracked as a framework-level follow-up, not a
// `Dismissible`-level one — the plain (never-dragged) unmount case above
// proves `dispose` itself is correctly wired.
