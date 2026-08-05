//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/mouse_region_test.dart`
//! (tag `3.44.0`, 43 `testWidgets` cases — verified via `git show
//! 3.44.0:packages/flutter/test/widgets/mouse_region_test.dart`). This is
//! `MouseRegion`'s own oracle file — the last input-family widget without a
//! parity file (`Listener`/`AbsorbPointer`/`IgnorePointer` landed in
//! `pointer_hit_test_test.rs`; that file's own module doc left `MouseRegion`
//! explicitly out of scope pending a device-tracking harness capability that
//! this file re-checks rather than assumes).
//!
//! ### Harness capabilities checked before writing this file
//!
//! - **No device connect/disconnect primitive.** `PointerEvent` has exactly
//!   four kinds — `Down`/`Move`/`Up`/`Cancel`
//!   (`crates/flui-interaction/src/events.rs:913-922`); there is no
//!   `Added`/`Removed` kind analogous to Flutter's `PointerAddedEvent`/
//!   `PointerRemovedEvent`, and `MouseTracker::add_device`/`remove_device`
//!   (`crates/flui-interaction/src/routing/mouse_tracker.rs:180-205`) are
//!   never called from the pointer-dispatch pipeline
//!   (`crates/flui-interaction/src/binding.rs`) — only from
//!   `#[cfg(test)]` unit tests inside `mouse_tracker.rs` itself. A "mouse
//!   connects/disconnects" event cannot be synthesized through
//!   [`LaidOut`](crate::common::LaidOut)'s public dispatch surface.
//! - **`dispatch_pointer_hover` hardcoded `PointerType::Mouse`.** There was no
//!   `LaidOut` method for a non-mouse hover device, so this file's harness
//!   change adds `LaidOut::dispatch_pointer_hover_with_kind` (device-kind
//!   parameter; `dispatch_pointer_hover` now just calls it with `Mouse`) to
//!   reach the one case that needs it — see
//!   [`stylus_hover_delivers_enter_hover_exit_like_a_mouse`].
//! - **No paint-count, compositing-layer, cursor, or `hasScheduledFrame`
//!   introspection.** Same category of gap `transform_test.rs` and
//!   `clip_test.rs` already document for `TransformLayer`/`paints()`
//!   assertions — `LaidOut` exposes geometry and dispatch, not a paint
//!   recorder, a compositing-layer tree, `MouseTracker::device_cursor`
//!   (exists but not surfaced — the `binding` field is private), or a
//!   scheduled-frame flag.
//!
//! ### A genuine FLUI/Flutter divergence found while classifying, and fixed
//! in this file's own change — reported per this file's honesty bar, not
//! silently ported around: Flutter's `RenderMouseRegion.handleEvent`
//! (`rendering/proxy_box.dart` 3.44.0) fires `onHover` for **any**
//! `PointerHoverEvent` reaching it as an ordinary `HitTestTarget`, regardless
//! of device kind — independent of `MouseTracker`, which separately gates
//! enter/exit to `mouse`/`stylus` only (`rendering/mouse_tracker.dart:302`).
//! FLUI's `RenderMouseRegion`
//! (`crates/flui-objects/src/interaction/mouse_region.rs`) has no generic
//! hit-test-event handler at all — it contributes only a
//! [`MouseTrackerAnnotation`]. `on_hover` used to fire **exclusively**
//! through `MouseTracker::update_with_motion`, which is gated to
//! `Mouse | Pen` (`mouse_tracker.rs:220-222`) — the same gate Flutter uses
//! for enter/exit, but Flutter does not apply it to hover — so a touch-kind
//! hover delivered `on_hover` in Flutter and nothing at all in FLUI.
//!
//! The fix relocates `on_hover` delivery out of the device-state machine
//! entirely, with no device-kind gate — mirroring `handleEvent` exactly —
//! and rides the ordinary coalesced pointer-move dispatch path rather than
//! the immediate per-raw-event enter/exit update.
//! `MouseTracker::update_with_motion` keeps its `Mouse | Pen` gate, now
//! scoped to enter/exit/cursor only, matching
//! `MouseTracker.updateWithEvent`'s real scope in Flutter. See
//! [`detects_hover_from_touch_devices`], now ported below instead of listed
//! as a divergence.
//!
//! Production delivery resolves `on_hover` together with ordinary pointer
//! targets in one leaf-first pass
//! (`InteractionDispatchHandle::dispatch_hover_interleaved`,
//! `crates/flui-interaction/src/routing/interaction_lane.rs`) rather than
//! calling `MouseTracker::dispatch_hover` as a separate full pass — a
//! `Listener` and a nested `MouseRegion` must fire in hit-test order
//! relative to each other, and two independent passes always deliver every
//! ordinary target before every region regardless of which is actually the
//! leaf. See
//! [`hover_fires_leaf_first_through_a_listener_wrapping_a_mouse_region`] and
//! [`hover_fires_leaf_first_through_a_mouse_region_wrapping_a_listener`].
//! `MouseTracker::dispatch_hover` itself is unchanged and stays public as
//! the self-contained, directly testable form of the same resolve-and-invoke
//! step.
//!
//! ### Stationary-device postframe recheck, now wired into the headless
//! frame path
//!
//! `MouseTracker::update_all_devices` — the mechanism that re-hit-tests
//! every tracked device after a frame that changed the tree shape, with no
//! new pointer motion (Flutter's implicit `MouseTracker.updateAllDevices`
//! postframe recheck, `rendering/mouse_tracker.dart:366`) — used to be
//! called exactly once in the whole workspace:
//! `crates/flui-app/src/app/binding.rs`, inside the production frame path.
//! `flui-testing`'s `HeadlessBinding::pump_frame` (what
//! `LaidOut::pump_widget`/`pump`/`tick` drive) never called it, so this
//! widget-test harness could not observe or prove Flutter's "widget moves
//! under a stationary pointer ⇒ automatic enter/exit" contract even though
//! production reproduced it correctly. `pump_frame` now runs the same
//! recheck, every frame, unconditionally, for a tree-bound binding —
//! against its own `PipelineOwner` directly (the tree-bound branch already
//! owns the same `PipelineCell` production's `hit_test_in_view` wraps, so
//! no caller-supplied hit-test closure needed adding to `pump_frame`'s
//! signature). See the doc on
//! [`HeadlessBinding::pump_frame`](flui_testing::HeadlessBinding::pump_frame)
//! for the full ordering.
//!
//! Wiring the recheck surfaced a real, pre-existing bug, independent of this
//! file's own scope but directly exposed by exercising the recheck for the
//! first time: [`removing_a_hovered_region_does_not_synthesize_an_exit`]
//! (already ported, previously passing only because nothing rechecked a
//! stationary device) started failing — a removed, still-hovered region's
//! stale `on_exit` fired spuriously. The root cause was two independent
//! copies of the same ordering bug, in `ElementTree::remove_subtree`
//! (`crates/flui-view/src/tree/element_tree.rs`) and `id_reconcile::remove_child`
//! (`crates/flui-view/src/tree/id_reconcile.rs`, the path an ordinary
//! `pump_widget` root-swap actually takes): both unmounted the root of a
//! removed subtree *before* its descendants, and the root's own
//! render-object removal cascades (`PipelineOwner::remove_render_object`
//! recurses over descendants), deleting every descendant's render object
//! before that descendant's own `unmount` — and therefore its
//! `RenderView::did_unmount_render_object` hook, which is how
//! `MouseRegion`/`ClipPath`/`Listener` unregister from the owner-local
//! interaction lane — ever ran. The hook was silently skipped for any
//! interaction-lane-registering widget nested under a removed ancestor, not
//! just `MouseRegion`: a permanent registration leak, not merely a
//! mouse-tracker quirk. Both functions now unmount descendants deepest-first
//! *before* the (unkeyed) root, mirroring Flutter's own bottom-up
//! `RenderObject.detach()` walk; a keyed root's existing soft-remove
//! (parked for `finalize_tree`, descendants untouched) is unchanged.
//!
//! A second, narrower fix was needed on top of the reorder: `MouseTracker`
//! caches a strong `Rc` clone of a mouse-region target's callback cell (its
//! `annotations` map, keyed by region id) across frames, so it can resolve
//! an exit callback for a region no longer present in a fresh hit test. A
//! permanently unmounted region's render object is gone from the tree for
//! good, so that cached clone must never fire again once unmount runs —
//! `InteractionDispatchHandle`/`RenderObjectContext` gained a narrower call,
//! `detach_mouse_region`, that invalidates the shared cell's callbacks in
//! place (not just drops the lane's own map entry, which the general-purpose
//! `unregister_mouse_region` does); `RenderView::did_unmount_render_object`
//! for `MouseRegion` calls it. This is FLUI's structural equivalent of
//! Flutter's `validForMouseTracker` flag (`rendering/proxy_box.dart`
//! `RenderMouseRegion.detach`), which exists for exactly this reason.
//!
//! A THIRD bug surfaced once the recheck ran against a still-mounted
//! region, found by measuring the pointer state across an empty-callback
//! rebuild rather than assuming the unmount-order fix above was the whole
//! story: `MouseRegion::sync_mouse_region_target`'s original zero-callbacks
//! branch called the general-purpose `unregister_mouse_region` — which
//! drops the lane's own map entry but leaves the shared cell's contents
//! untouched — and cleared the render object's own `mouse_region_target`.
//! With no target, `RenderMouseRegion::mouse_tracker_annotation`
//! (`crates/flui-objects/src/interaction/mouse_region.rs`) stops
//! contributing an annotation at all, so the very next postframe recheck's
//! hit test simply does not find the region — indistinguishable, from the
//! tracker's point of view, from the region having moved away or been
//! removed. The tracker resolved its still callback-intact cached cell and
//! fired a stale `on_exit`, even though the region was never removed from
//! the tree and the pointer never moved. Flutter has no such failure mode:
//! `RenderMouseRegion.onEnter`/`onHover`/`onExit` are plain nullable fields
//! on a render object that stays a valid `MouseTrackerAnnotation` for as
//! long as it is attached (`rendering/proxy_box.dart`) — there is no
//! "unregister while still attached" step to get wrong. The fix ports that
//! shape: a still-mounted region now keeps its lane target for its whole
//! mounted lifetime and replaces its callbacks with the empty set instead
//! of dropping the registration — see
//! [`rebuilding_a_hovered_region_down_to_no_callbacks_fires_nothing`],
//! which pins the corrected behavior (confirmed empirically: reverting only
//! the widget-level fix, with the recheck and the unmount-order fix both
//! still in place, reintroduces the stale exit).
//!
//! ### Ported (19 of 43)
//! - `'hitTestBehavior test - HitTestBehavior.deferToChild/opaque'` —
//!   [`hit_test_behavior_defer_to_child_then_opaque_toggles_enter`].
//! - `'hitTestBehavior test - HitTestBehavior.deferToChild and non-opaque'` —
//!   [`hit_test_behavior_defer_to_child_non_opaque_does_not_block_the_region_below`].
//! - `'hitTestBehavior test - HitTestBehavior.translucent'` —
//!   [`hit_test_behavior_translucent_does_not_block_the_region_below`].
//! - `'onEnter and onExit can be triggered with mouse buttons pressed'`
//!   (adapted: FLUI's `on_enter`/`on_hover`/`on_exit` deliver a single
//!   `Offset` — see the render-object's `MouseRegionCallback` type alias,
//!   `crates/flui-objects/src/interaction/mouse_region.rs:22` — which is the
//!   raw event position (`DeviceWork::invoke`,
//!   `mouse_tracker.rs:543-609`, calls every callback with the same
//!   `self.position`), never a region-local position; the upstream
//!   `enter!.localPosition`/`exit!.localPosition` assertions have nothing to
//!   port against, so only `.position` is checked) —
//!   [`enter_and_exit_fire_while_a_mouse_button_is_held`].
//! - `'detects pointer enter'` — [`detects_pointer_enter`].
//! - `'detects pointer exiting'` — [`detects_pointer_exiting`].
//! - `"doesn't trigger pointer exit when widget disappears"` —
//!   [`removing_a_hovered_region_does_not_synthesize_an_exit`].
//! - `'Hover works with nested listeners'` —
//!   [`hover_works_with_nested_regions`].
//! - `'Hover transfers between two listeners'` —
//!   [`hover_transfers_between_two_regions`].
//! - `'MouseRegion uses updated callbacks'` —
//!   [`mouse_region_uses_updated_callbacks`].
//! - `'works with transform'` (the geometry legs only — the `delta` field
//!   assertion has no FLUI equivalent for the same single-`Offset`-callback
//!   reason as the mouse-buttons-pressed case above; substituted with an
//!   equally strong direct check that the delivered `Offset` matches the
//!   dispatched global position) —
//!   [`hit_test_transitions_correctly_through_a_2x_transform_scale`]. This is
//!   the highest-value port in this file: the hit-test transform stack
//!   composes global→local as the walk descends, so the position a region
//!   receives must be local to it, not an ancestor's untransformed global
//!   position. `pointer_local_position_test.rs` proves that invariant at the
//!   lower-level hit-test/dispatch plumbing directly (`Listener` under
//!   scaled/rotated ancestors); this case proves it holds end-to-end through
//!   `MouseRegion`'s widget → render-object wiring too.
//! - `"Callbacks aren't called during build"` (adapted: Flutter's oracle
//!   drives the rebuild through an internal `setState` inside the
//!   `HoverClient`/`HoverFeedback` `StatefulWidget`s that own the callbacks;
//!   wiring a FLUI `StatefulView`'s rebuild handle into a callback registered
//!   from `build()` is real but disproportionate plumbing for this single
//!   case, so this port drives the exact same rebuild — a `pump_widget`
//!   root-swap with no new pointer dispatch between it and the prior
//!   dispatch — from the test body directly. The invariant under test
//!   (rebuilding an already-hovered/already-exited region must not refire
//!   `on_enter`/`on_exit`) is identical either way) —
//!   [`rebuilding_an_active_region_does_not_refire_enter_or_exit`].
//! - `'opaque should default to true'` (`group('MouseRegion respects
//!   opacity:', ...)`) —
//!   [`opaque_defaults_to_true_and_blocks_the_region_below`].
//! - `'an empty opaque MouseRegion is effective'` —
//!   [`a_childless_opaque_mouse_region_still_blocks_everything_beneath_it`].
//! - `'stylus input works'` (via
//!   `LaidOut::dispatch_pointer_hover_with_kind(.., PointerType::Pen)` —
//!   see the harness-capabilities note above) —
//!   [`stylus_hover_delivers_enter_hover_exit_like_a_mouse`].
//! - `'detects hover from touch devices'` (via
//!   `LaidOut::dispatch_pointer_hover_with_kind(.., PointerType::Touch)`) —
//!   the divergence fix above made this portable; see the section on it —
//!   [`detects_hover_from_touch_devices`].
//! - `'triggers pointer enter when widget appears'` — now reachable via the
//!   postframe-recheck wiring above —
//!   [`stationary_pointer_over_a_newly_appeared_region_triggers_enter_next_frame`].
//! - `'triggers pointer enter when widget moves in'` —
//!   [`stationary_pointer_over_a_region_that_moves_in_triggers_enter_next_frame`].
//! - `'triggers pointer exit when widget moves out'` (its initial "enter"
//!   leg, which upstream reaches by connecting a fresh mouse device, is
//!   replaced by an ordinary dispatched hover — no device connect/disconnect
//!   primitive exists in this harness, per the capability note above; the
//!   "moves out while stationary" leg under test is unaffected) —
//!   [`stationary_pointer_over_a_region_that_moves_out_triggers_exit_next_frame`].
//!
//! ### Not ported (24 of 43) {#not-ported}
//! Grouped by shared reason so the arithmetic stays checkable (19 + 24 = 43)
//! without repeating each explanation 43 times:
//!
//! **No device connect/disconnect primitive** (3): `'triggers pointer enter
//! when a mouse is connected'`, `'triggers pointer exit when a mouse is
//! disconnected'`, `'Exit event when unplugging mouse should have a
//! position'`.
//!
//! **`hasScheduledFrame` has no harness equivalent** (2): `'A MouseRegion
//! unmounted under the pointer should not trigger state change'`, `'No new
//! frames are scheduled when mouse moves without triggering callbacks'`.
//!
//! **No cursor introspection surfaced by `LaidOut`** (1) —
//! `MouseTracker::device_cursor` exists (`mouse_tracker.rs:450`) but nothing
//! exposes it through the widget-test harness (`LaidOut`'s `binding` field is
//! private); a legitimate small harness extension, out of scope for a
//! test-porting pass: `'applies mouse cursor'`.
//!
//! **No compositing-layer/`needsCompositing`/layer-count introspection** (2)
//! — the same gap `transform_test.rs`'s own module doc documents for
//! `TransformLayer` assertions: `'needsCompositing set when parent class
//! needsCompositing is set'`, `'needsCompositing is always false'`.
//!
//! **No paint-count introspection** (6) — `LaidOut` has no `CustomPaint`
//! call-count recorder: `'MouseRegion paints child once and only once when
//! MouseRegion is inactive'`, `'... when MouseRegion is active'`, `"Changing
//! MouseRegion's callbacks is effective and doesn't repaint"`, `'Changing
//! MouseRegion.opaque is effective and repaints'`, `'Changing
//! MouseRegion.cursor is effective and repaints'` (also needs cursor
//! introspection, above), `'Changing whether MouseRegion.cursor is null is
//! effective and repaints'` (same, also needs cursor introspection).
//!
//! **Diagnostics-string formatting, not hit-test behavior** (2) — same
//! out-of-scope call `pointer_hit_test_test.rs`'s module doc already makes
//! for `Listener`'s `debugFillProperties` cases: `"RenderMouseRegion's
//! debugFillProperties when default"`, `"RenderMouseRegion's
//! debugFillProperties when full"`.
//!
//! **`StatefulView`-rebuild-handle-in-callback plumbing, deferred** (4) —
//! same real-but-disproportionate cost as the one case adapted around above
//! ([`rebuilding_an_active_region_does_not_refire_enter_or_exit`]), but these
//! four don't clear the "worth the substitution" bar the way that one does:
//! `'MouseRegion activate/deactivate don't duplicate annotations'` (also
//! needs `GlobalKey`-based deactivate/reactivate), `'detects pointer enter
//! with closure arguments'` (tests no `MouseRegion` contract beyond
//! [`detects_pointer_enter`]/[`detects_pointer_exiting`]/
//! [`removing_a_hovered_region_does_not_synthesize_an_exit`] once the
//! `StatefulWidget` indirection is stripped away), and two cases the
//! postframe-recheck wiring above made only *partially* reachable —
//! `'A MouseRegion mounted under the pointer should take effect in the next
//! postframe'` and `'A MouseRegion moved into the mouse should take effect
//! in the next postframe'`. Upstream drives both through `HoverClient`, a
//! `StatefulWidget` whose `onHover` callback calls `setState`, so the
//! specific thing under test — that the state change (and its dependent
//! text) lands on the *next* frame, not the one that mounted/moved the
//! region, with no further frame scheduled — needs a real rebuild triggered
//! *from inside* the enter callback. A bare `Cell`/`RefCell` counter (as
//! [`stationary_pointer_over_a_newly_appeared_region_triggers_enter_next_frame`]
//! and
//! [`stationary_pointer_over_a_region_that_moves_in_triggers_enter_next_frame`]
//! use) only proves the callback fired on the right frame, which those two
//! cases already cover; it cannot prove the callback-driven second rebuild
//! landed on ITS OWN next frame rather than immediately, since a headless
//! `pump_widget` call would drive both in the same call regardless. Also
//! still blocked on `hasScheduledFrame`, above, which both cases assert on
//! directly.
//!
//! **`GlobalKey` reparent does not preserve render-object identity in
//! FLUI** (1) — `'Does not trigger side effects during a reparent'` depends
//! on the *same* `RenderMouseRegion`/mouse-tracker annotation surviving a
//! structural move so no cursor-platform-channel call re-fires.
//! `animated_size_test.rs`'s own module doc already documents this
//! architectural divergence for `AnimatedSize`: "FLUI's remove+insert
//! reparent model cannot preserve a Rust render object's identity across
//! that boundary." A reparented `MouseRegion` mints a fresh
//! `RenderMouseRegion`, tearing down the old mouse-tracker annotation and
//! registering a new one within the same `pump_widget` call, with no pointer
//! event dispatched in between — porting this case would only re-prove "no
//! dispatch ⇒ no spurious event," which
//! [`removing_a_hovered_region_does_not_synthesize_an_exit`] already covers;
//! it would not prove what the Flutter test's name claims.
//!
//! **Disproportionate regression-scenario setup, not a `MouseRegion`
//! capability gap** (1) — `'Handle mouse events should ignore the detached
//! MouseTrackerAnnotation'` exercises a `Draggable`-drag-causes-mid-gesture-
//! detachment regression; `Draggable`'s own parity lives in
//! `draggable_test.rs`, and this specific detached-annotation scenario is
//! large new setup for a case outside this file's `MouseRegion`-behavior
//! scope.
//!
//! **Deliberate scope cut, not a capability gap** (2) — `group('MouseRegion
//! respects opacity:', ...)`'s other two cases, `'a transparent one should
//! allow MouseRegions behind it to receive pointers'` and `'an opaque one
//! should prevent MouseRegions behind it receiving pointers'`, replay the
//! same three-way-stack + opaque/non-opaque hit-test contract the three
//! `hitTestBehavior` cases and
//! [`opaque_defaults_to_true_and_blocks_the_region_below`] already establish,
//! across six sequential move legs of cumulative order-sensitive log
//! assertions instead of two. Valuable as an exhaustive transition matrix;
//! disproportionate additional scope for this pass.
//!
//! Count check, one primary reason per case, no case counted twice: 3
//! (connect/disconnect) + 2 (`hasScheduledFrame`) + 1 (cursor introspection) +
//! 2 (compositing) + 6 (paint-count) + 2 (`debugFillProperties`) + 4
//! (`StatefulView` plumbing) + 1 (`GlobalKey` reparent) + 1 (`Draggable`
//! regression) + 2 (opacity transition matrix) =
//! 24 not ported. 19 ported + 24 not ported = 43.
//!
//! Widget → render-object mapping:
//! - `MouseRegion` → `RenderMouseRegion`
//!   (`crates/flui-objects/src/interaction/mouse_region.rs`)
//! - Enter/hover/exit delivery → `MouseTracker`
//!   (`crates/flui-interaction/src/routing/mouse_tracker.rs`)
//!
//! This file is additive to `crates/flui-widgets/tests/mouse_region.rs` (3
//! non-parity tests: childless fill/sizes-to-child geometry, a single
//! `on_hover` smoke test) and `crates/flui-objects/tests/render_object_harness.rs`
//! (`:1021`/`:1042`, `RenderMouseRegion` sibling/tracker entries at the
//! render-object level).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_interaction::events::PointerType;
use flui_types::{Alignment, Color, Offset};
use flui_view::ViewExt;
use flui_widgets::prelude::*;

use crate::harness;

fn offset(x: f32, y: f32) -> Offset {
    Offset::new(px(x), px(y))
}

// ── hitTestBehavior ──────────────────────────────────────────────────────────

/// A childless `DeferToChild` region never registers a hit at all (no child
/// to defer to); switching the SAME region to the default `Opaque` behavior
/// makes it self-register unconditionally within its own bounds.
///
/// Flutter parity: `'hitTestBehavior test - HitTestBehavior.deferToChild/opaque'`
/// — `MouseRegion(hitTestBehavior: deferToChild)` under a stationary pointer
/// never fires `onEnter`; rebuilding to the default `MouseRegion(onEnter:
/// ...)` (opaque) does. Adapted: upstream observes the second leg's `onEnter`
/// from the *stationary* pointer alone (Flutter's implicit postframe
/// recheck); this harness has no equivalent (see the module doc), so the
/// second leg re-dispatches an explicit hover at the same point instead —
/// still a genuine, fresh hit test the same device performs on receiving a
/// real event, not a synthesized pass.
#[test]
fn hit_test_behavior_defer_to_child_then_opaque_toggles_enter() {
    let entered = Rc::new(Cell::new(false));
    let on_enter = Rc::clone(&entered);

    let mut laid = harness::pump_widget(
        MouseRegion::new()
            .behavior(HitTestBehavior::DeferToChild)
            .on_enter(move |_device, _position| on_enter.set(true)),
        harness::screen_of(50.0, 50.0),
    );
    laid.dispatch_pointer_hover(25.0, 25.0);
    assert!(
        !entered.get(),
        "a childless DeferToChild region has no child to defer to and must never register a hit"
    );

    let on_enter = Rc::clone(&entered);
    laid.pump_widget(MouseRegion::new().on_enter(move |_device, _position| on_enter.set(true)));
    laid.dispatch_pointer_hover(25.0, 25.0);
    assert!(
        entered.get(),
        "the default Opaque behavior must self-register a hit even with no child"
    );
}

/// A `deferToChild`, non-opaque region with a real hit-testable child does
/// not block the sibling MouseRegion stacked below it.
///
/// Flutter parity: `'hitTestBehavior test - HitTestBehavior.deferToChild and
/// non-opaque'` — two overlapping `MouseRegion`s in a `Stack`; the top one
/// (`opaque: false`, `deferToChild`, with a real child) still lets the
/// bottom one see the pointer.
#[test]
fn hit_test_behavior_defer_to_child_non_opaque_does_not_block_the_region_below() {
    let region1_entered = Rc::new(Cell::new(false));
    let region2_entered = Rc::new(Cell::new(false));
    let on_enter1 = Rc::clone(&region1_entered);
    let on_enter2 = Rc::clone(&region2_entered);

    let laid = harness::pump_widget(
        Stack::new(vec![
            SizedBox::new(50.0, 50.0)
                .child(MouseRegion::new().on_enter(move |_d, _p| on_enter1.set(true)))
                .boxed(),
            SizedBox::new(50.0, 50.0)
                .child(
                    MouseRegion::new()
                        .opaque(false)
                        .behavior(HitTestBehavior::DeferToChild)
                        .on_enter(move |_d, _p| on_enter2.set(true))
                        .child(
                            ColoredBox::new(Color::rgb(255, 16, 25))
                                .child(SizedBox::new(50.0, 50.0)),
                        ),
                )
                .boxed(),
        ]),
        harness::screen_of(50.0, 50.0),
    );

    laid.dispatch_pointer_hover(10.0, 10.0);

    assert!(
        region2_entered.get(),
        "the top, deferToChild region has a hit-testable child and must self-register"
    );
    assert!(
        region1_entered.get(),
        "opaque(false) on the top region must let the bottom sibling see the pointer too"
    );
}

/// A childless `translucent` region self-registers (unlike `deferToChild`)
/// and, being non-opaque by construction, does not block the sibling below.
///
/// Flutter parity: `'hitTestBehavior test - HitTestBehavior.translucent'`.
#[test]
fn hit_test_behavior_translucent_does_not_block_the_region_below() {
    let region1_entered = Rc::new(Cell::new(false));
    let region2_entered = Rc::new(Cell::new(false));
    let on_enter1 = Rc::clone(&region1_entered);
    let on_enter2 = Rc::clone(&region2_entered);

    let laid = harness::pump_widget(
        Stack::new(vec![
            SizedBox::new(50.0, 50.0)
                .child(MouseRegion::new().on_enter(move |_d, _p| on_enter1.set(true)))
                .boxed(),
            SizedBox::new(50.0, 50.0)
                .child(
                    MouseRegion::new()
                        .behavior(HitTestBehavior::Translucent)
                        .on_enter(move |_d, _p| on_enter2.set(true)),
                )
                .boxed(),
        ]),
        harness::screen_of(50.0, 50.0),
    );

    laid.dispatch_pointer_hover(10.0, 10.0);

    assert!(
        region2_entered.get(),
        "a childless translucent region must self-register even with nothing to defer to"
    );
    assert!(
        region1_entered.get(),
        "translucent must not block the sibling below"
    );
}

// ── Enter / hover / exit ─────────────────────────────────────────────────────

/// Enter and exit still fire while a mouse button is held (a `Move` inside an
/// active Down-to-Up sequence, not a bare `Hover`).
///
/// Flutter parity: `'onEnter and onExit can be triggered with mouse buttons
/// pressed'`. See the module doc for why only `.position` (not
/// `.localPosition`) is checked.
#[test]
fn enter_and_exit_fire_while_a_mouse_button_is_held() {
    let enters: Rc<RefCell<Vec<Offset>>> = Rc::new(RefCell::new(Vec::new()));
    let exits: Rc<RefCell<Vec<Offset>>> = Rc::new(RefCell::new(Vec::new()));
    let on_enter = Rc::clone(&enters);
    let on_exit = Rc::clone(&exits);

    let laid = harness::pump_widget(
        Align::new(Alignment::CENTER).child(
            MouseRegion::new()
                .on_enter(move |_device, position| on_enter.borrow_mut().push(position))
                .on_exit(move |_device, position| on_exit.borrow_mut().push(position))
                .child(SizedBox::new(100.0, 100.0)),
        ),
        harness::screen_of(300.0, 300.0),
    );

    // Press the button outside the centered 100x100 box (spans (100,100)-(200,200)).
    laid.dispatch_pointer_down(10.0, 10.0);
    assert!(enters.borrow().is_empty());

    laid.dispatch_pointer_move(150.0, 150.0);
    assert_eq!(
        enters.borrow().as_slice(),
        &[offset(150.0, 150.0)],
        "a pressed move into the region must still fire onEnter"
    );
    assert!(exits.borrow().is_empty());

    laid.dispatch_pointer_move(10.0, 10.0);
    assert_eq!(
        exits.borrow().as_slice(),
        &[offset(10.0, 10.0)],
        "a pressed move back out must still fire onExit"
    );

    laid.dispatch_pointer_up(10.0, 10.0);
}

/// A hover into a region fires `enter` then `hover`, in that order, with no
/// `exit`.
///
/// Flutter parity: `'detects pointer enter'`.
#[test]
fn detects_pointer_enter() {
    let log: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
    let (on_enter, on_hover, on_exit) = (Rc::clone(&log), Rc::clone(&log), Rc::clone(&log));

    let laid = harness::pump_widget(
        Align::new(Alignment::CENTER).child(
            MouseRegion::new()
                .on_enter(move |_d, _p| on_enter.borrow_mut().push("enter"))
                .on_hover(move |_d, _p| on_hover.borrow_mut().push("hover"))
                .on_exit(move |_d, _p| on_exit.borrow_mut().push("exit"))
                .child(SizedBox::new(100.0, 100.0)),
        ),
        harness::screen_of(300.0, 300.0),
    );

    laid.dispatch_pointer_hover(150.0, 150.0);

    assert_eq!(log.borrow().as_slice(), &["enter", "hover"]);
}

/// A hover that leaves a region fires only `exit`.
///
/// Flutter parity: `'detects pointer exiting'`.
#[test]
fn detects_pointer_exiting() {
    let log: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
    let (on_enter, on_hover, on_exit) = (Rc::clone(&log), Rc::clone(&log), Rc::clone(&log));

    let laid = harness::pump_widget(
        Align::new(Alignment::CENTER).child(
            MouseRegion::new()
                .on_enter(move |_d, _p| on_enter.borrow_mut().push("enter"))
                .on_hover(move |_d, _p| on_hover.borrow_mut().push("hover"))
                .on_exit(move |_d, _p| on_exit.borrow_mut().push("exit"))
                .child(SizedBox::new(100.0, 100.0)),
        ),
        harness::screen_of(300.0, 300.0),
    );

    laid.dispatch_pointer_hover(150.0, 150.0);
    log.borrow_mut().clear();

    laid.dispatch_pointer_hover(10.0, 10.0);

    assert_eq!(log.borrow().as_slice(), &["exit"]);
}

/// Removing an actively-hovered region (a `pump_widget` root-swap, no new
/// pointer dispatch) fires no spurious `exit` — the unmount path
/// unregisters the mouse-tracker annotation silently.
///
/// Flutter parity: `"doesn't trigger pointer exit when widget disappears"`.
#[test]
fn removing_a_hovered_region_does_not_synthesize_an_exit() {
    let log: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
    let (on_enter, on_hover, on_exit) = (Rc::clone(&log), Rc::clone(&log), Rc::clone(&log));

    let mut laid = harness::pump_widget(
        Align::new(Alignment::CENTER).child(
            MouseRegion::new()
                .on_enter(move |_d, _p| on_enter.borrow_mut().push("enter"))
                .on_hover(move |_d, _p| on_hover.borrow_mut().push("hover"))
                .on_exit(move |_d, _p| on_exit.borrow_mut().push("exit"))
                .child(SizedBox::new(100.0, 100.0)),
        ),
        harness::screen_of(300.0, 300.0),
    );

    laid.dispatch_pointer_hover(150.0, 150.0);
    log.borrow_mut().clear();

    laid.pump_widget(SizedBox::new(100.0, 100.0));

    assert!(
        log.borrow().is_empty(),
        "removing a hovered region with no new pointer dispatch must not synthesize an exit"
    );
}

/// A pointer over a nested region fires enter/hover on both the inner and
/// outer regions; moving to a point still within the outer region but
/// outside the inner one exits only the inner region (the outer keeps
/// hovering, no re-enter, no exit).
///
/// Flutter parity: `'Hover works with nested listeners'` — outer
/// `MouseRegion(200x200, padding 50)` wrapping an inner `MouseRegion`
/// (fills the padded 100x100 remainder).
#[test]
fn hover_works_with_nested_regions() {
    let outer_log: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
    let inner_log: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
    let (outer_enter, outer_hover, outer_exit) = (
        Rc::clone(&outer_log),
        Rc::clone(&outer_log),
        Rc::clone(&outer_log),
    );
    let (inner_enter, inner_hover, inner_exit) = (
        Rc::clone(&inner_log),
        Rc::clone(&inner_log),
        Rc::clone(&inner_log),
    );

    let laid = harness::pump_widget(
        MouseRegion::new()
            .on_enter(move |_d, _p| outer_enter.borrow_mut().push("enter"))
            .on_hover(move |_d, _p| outer_hover.borrow_mut().push("hover"))
            .on_exit(move |_d, _p| outer_exit.borrow_mut().push("exit"))
            .child(
                Container::new().padding(EdgeInsets::all(px(50.0))).child(
                    MouseRegion::new()
                        .on_enter(move |_d, _p| inner_enter.borrow_mut().push("enter"))
                        .on_hover(move |_d, _p| inner_hover.borrow_mut().push("hover"))
                        .on_exit(move |_d, _p| inner_exit.borrow_mut().push("exit")),
                ),
            ),
        harness::screen_of(200.0, 200.0),
    );

    // Inner region spans (50,50)-(150,150); center (100,100).
    laid.dispatch_pointer_hover(100.0, 100.0);
    assert_eq!(inner_log.borrow().as_slice(), &["enter", "hover"]);
    assert_eq!(outer_log.borrow().as_slice(), &["enter", "hover"]);
    inner_log.borrow_mut().clear();
    outer_log.borrow_mut().clear();

    // Still within the outer 200x200 box, outside the inner 100x100 one.
    laid.dispatch_pointer_hover(25.0, 100.0);
    assert_eq!(
        inner_log.borrow().as_slice(),
        &["exit"],
        "leaving the inner region only exits the inner region"
    );
    assert_eq!(
        outer_log.borrow().as_slice(),
        &["hover"],
        "the outer region keeps hovering, with no re-enter or exit"
    );
}

/// Hover transfers cleanly between two sibling regions: entering the second
/// exits the first, and moving off both exits whichever was last active.
/// Unmounting afterward (no new dispatch) fires nothing further.
///
/// Flutter parity: `'Hover transfers between two listeners'`.
#[test]
fn hover_transfers_between_two_regions() {
    let log1: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
    let log2: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
    let (e1, h1, x1) = (Rc::clone(&log1), Rc::clone(&log1), Rc::clone(&log1));
    let (e2, h2, x2) = (Rc::clone(&log2), Rc::clone(&log2), Rc::clone(&log2));

    let mut laid = harness::pump_widget(
        Column::new(vec![
            MouseRegion::new()
                .on_enter(move |_d, _p| e1.borrow_mut().push("enter"))
                .on_hover(move |_d, _p| h1.borrow_mut().push("hover"))
                .on_exit(move |_d, _p| x1.borrow_mut().push("exit"))
                .child(SizedBox::new(100.0, 100.0))
                .boxed(),
            MouseRegion::new()
                .on_enter(move |_d, _p| e2.borrow_mut().push("enter"))
                .on_hover(move |_d, _p| h2.borrow_mut().push("hover"))
                .on_exit(move |_d, _p| x2.borrow_mut().push("exit"))
                .child(SizedBox::new(100.0, 100.0))
                .boxed(),
        ]),
        harness::screen_of(100.0, 200.0),
    );

    // Region 1 spans y in [0,100), region 2 spans y in [100,200).
    laid.dispatch_pointer_hover(50.0, 50.0);
    assert_eq!(log1.borrow().as_slice(), &["enter", "hover"]);
    assert!(log2.borrow().is_empty());
    log1.borrow_mut().clear();

    laid.dispatch_pointer_hover(50.0, 150.0);
    assert_eq!(log1.borrow().as_slice(), &["exit"]);
    assert_eq!(log2.borrow().as_slice(), &["enter", "hover"]);
    log1.borrow_mut().clear();
    log2.borrow_mut().clear();

    // Outside the 200-tall screen entirely -- outside both regions.
    laid.dispatch_pointer_hover(50.0, 205.0);
    assert!(log1.borrow().is_empty());
    assert_eq!(log2.borrow().as_slice(), &["exit"]);
    log2.borrow_mut().clear();

    laid.pump_widget(SizedBox::new(1.0, 1.0));
    assert!(log1.borrow().is_empty());
    assert!(
        log2.borrow().is_empty(),
        "removing both regions with no new dispatch must not synthesize an exit"
    );
}

/// Rebuilding a region with NEW callbacks routes subsequent enter/hover/exit
/// to the new closures, never the stale ones from mount.
///
/// Flutter parity: `'MouseRegion uses updated callbacks'`.
#[test]
fn mouse_region_uses_updated_callbacks() {
    fn region(
        log: &Rc<RefCell<Vec<&'static str>>>,
        enter: &'static str,
        hover: &'static str,
        exit: &'static str,
    ) -> Align {
        let (e, h, x) = (Rc::clone(log), Rc::clone(log), Rc::clone(log));
        Align::new(Alignment::TOP_LEFT).child(
            MouseRegion::new()
                .on_enter(move |_d, _p| e.borrow_mut().push(enter))
                .on_hover(move |_d, _p| h.borrow_mut().push(hover))
                .on_exit(move |_d, _p| x.borrow_mut().push(exit))
                .child(SizedBox::new(100.0, 100.0)),
        )
    }

    let log: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));

    let mut laid = harness::pump_widget(
        region(&log, "enter1", "hover1", "exit1"),
        harness::screen_of(300.0, 300.0),
    );

    laid.dispatch_pointer_hover(150.0, 150.0);
    assert!(
        log.borrow().is_empty(),
        "the pointer starts and stays outside the top-left 100x100 box"
    );

    laid.dispatch_pointer_hover(50.0, 50.0);
    laid.dispatch_pointer_hover(150.0, 150.0);
    assert_eq!(log.borrow().as_slice(), &["enter1", "hover1", "exit1"]);
    log.borrow_mut().clear();

    laid.pump_widget(region(&log, "enter2", "hover2", "exit2"));
    laid.dispatch_pointer_hover(50.0, 50.0);
    laid.dispatch_pointer_hover(150.0, 150.0);
    assert_eq!(
        log.borrow().as_slice(),
        &["enter2", "hover2", "exit2"],
        "rebuilt callbacks must be used, not the stale closures captured at mount"
    );
}

// ── Transform ─────────────────────────────────────────────────────────────

/// A `MouseRegion` scaled 2x by an ancestor `Transform` hit-tests correctly
/// against its SCALED bounds, not its unscaled layout bounds — proving the
/// hit-test-transform-composition fixes land correctly through the widget →
/// render-object wiring `MouseRegion` uses.
///
/// Geometry: the unscaled child (`SizedBox(150, 100)`) lays out at local
/// `(0,0)-(150,100)`; `Transform::scale` defaults to `Alignment::CENTER`
/// (matching Flutter's `Transform.scale` factory), pivoting on the child's
/// own center `(75, 50)`. The scaled absolute span is therefore `x:
/// [75-150, 75+150] = [-75, 225]`, `y: [50-100, 50+100] = [-50, 150]`.
///
/// Flutter parity: `'works with transform'` — the `delta`-field assertion
/// has no port (see the module doc); the equally strong substitute checks
/// the delivered `Offset` against the dispatched global position directly.
#[test]
fn hit_test_transitions_correctly_through_a_2x_transform_scale() {
    let events: Rc<RefCell<Vec<(&'static str, Offset)>>> = Rc::new(RefCell::new(Vec::new()));
    let (on_enter, on_hover, on_exit) =
        (Rc::clone(&events), Rc::clone(&events), Rc::clone(&events));

    let laid = harness::pump_widget(
        Transform::scale(2.0, 2.0).child(
            MouseRegion::new()
                .on_enter(move |_d, p| on_enter.borrow_mut().push(("enter", p)))
                .on_hover(move |_d, p| on_hover.borrow_mut().push(("hover", p)))
                .on_exit(move |_d, p| on_exit.borrow_mut().push(("exit", p)))
                .child(SizedBox::new(150.0, 100.0)),
        ),
        crate::common::loose(400.0),
    );

    // Just outside the scaled span's top-left corner (-75, -50).
    laid.dispatch_pointer_hover(-76.0, -51.0);
    assert!(events.borrow().is_empty());

    // Just inside the same corner.
    laid.dispatch_pointer_hover(-74.0, -49.0);
    assert_eq!(
        events.borrow().as_slice(),
        &[
            ("enter", offset(-74.0, -49.0)),
            ("hover", offset(-74.0, -49.0)),
        ]
    );
    events.borrow_mut().clear();

    // Just inside the scaled span's bottom-left corner (-75, 150).
    laid.dispatch_pointer_hover(-74.0, 149.0);
    assert_eq!(
        events.borrow().as_slice(),
        &[("hover", offset(-74.0, 149.0))]
    );
    events.borrow_mut().clear();

    // Just outside the same corner.
    laid.dispatch_pointer_hover(-74.0, 151.0);
    assert_eq!(
        events.borrow().as_slice(),
        &[("exit", offset(-74.0, 151.0))]
    );
}

// ── Build/rebuild safety ──────────────────────────────────────────────────

/// Rebuilding an already-hovered (or already-exited) region — with no new
/// pointer dispatch between the rebuild and the prior one — must not refire
/// `on_enter`/`on_exit`.
///
/// Flutter parity: `"Callbacks aren't called during build"`, adapted — see
/// the module doc for why this drives the rebuild directly instead of
/// through a `StatefulView`'s internal `setState`.
#[test]
fn rebuilding_an_active_region_does_not_refire_enter_or_exit() {
    let enters = Rc::new(Cell::new(0u32));
    let exits = Rc::new(Cell::new(0u32));

    fn region(enters: &Rc<Cell<u32>>, exits: &Rc<Cell<u32>>) -> MouseRegion {
        let (e, x) = (Rc::clone(enters), Rc::clone(exits));
        MouseRegion::new()
            .on_enter(move |_d, _p| e.set(e.get() + 1))
            .on_exit(move |_d, _p| x.set(x.get() + 1))
            .child(SizedBox::new(100.0, 100.0))
    }

    let mut laid = harness::pump_widget(region(&enters, &exits), harness::screen_of(100.0, 100.0));

    laid.dispatch_pointer_hover(50.0, 50.0);
    assert_eq!(enters.get(), 1);

    laid.pump_widget(region(&enters, &exits));
    assert_eq!(
        enters.get(),
        1,
        "a rebuild alone, with no new pointer event, must not refire enter"
    );
    assert_eq!(exits.get(), 0);

    laid.dispatch_pointer_hover(500.0, 500.0);
    assert_eq!(exits.get(), 1);

    laid.pump_widget(region(&enters, &exits));
    assert_eq!(
        exits.get(),
        1,
        "a rebuild after exit must not refire exit either"
    );
    assert_eq!(enters.get(), 1);
}

// ── Opacity ───────────────────────────────────────────────────────────────

/// Three overlapping regions (A wraps B and C; C sits on top of B and is
/// opaque by default): hitting the B/C overlap area fires A and C only (C
/// blocks B, whose own callbacks must never fire); moving out exits C then
/// A.
///
/// Flutter parity: `group('MouseRegion respects opacity:', ...)` →
/// `'opaque should default to true'`.
#[test]
fn opaque_defaults_to_true_and_blocks_the_region_below() {
    let log: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
    let (ea, ha, xa) = (Rc::clone(&log), Rc::clone(&log), Rc::clone(&log));
    let (eb, hb, xb) = (Rc::clone(&log), Rc::clone(&log), Rc::clone(&log));
    let (ec, hc, xc) = (Rc::clone(&log), Rc::clone(&log), Rc::clone(&log));

    let laid = harness::pump_widget(
        Align::new(Alignment::TOP_LEFT).child(
            MouseRegion::new()
                .on_enter(move |_d, _p| ea.borrow_mut().push("enterA"))
                .on_hover(move |_d, _p| ha.borrow_mut().push("hoverA"))
                .on_exit(move |_d, _p| xa.borrow_mut().push("exitA"))
                .child(SizedBox::new(150.0, 150.0).child(Stack::new(vec![
                        Positioned::new(
                            MouseRegion::new()
                                .on_enter(move |_d, _p| eb.borrow_mut().push("enterB"))
                                .on_hover(move |_d, _p| hb.borrow_mut().push("hoverB"))
                                .on_exit(move |_d, _p| xb.borrow_mut().push("exitB")),
                        )
                        .left(20.0)
                        .top(20.0)
                        .width(80.0)
                        .height(80.0)
                        .boxed(),
                        Positioned::new(
                            MouseRegion::new()
                                .on_enter(move |_d, _p| ec.borrow_mut().push("enterC"))
                                .on_hover(move |_d, _p| hc.borrow_mut().push("hoverC"))
                                .on_exit(move |_d, _p| xc.borrow_mut().push("exitC")),
                        )
                        .left(50.0)
                        .top(50.0)
                        .width(80.0)
                        .height(80.0)
                        .boxed(),
                    ]))),
        ),
        harness::screen_of(200.0, 200.0),
    );

    // (75,75) is inside A, B ([20,100]x[20,100]), and C ([50,130]x[50,130]);
    // C is stacked on top of B and opaque by default, so B never fires. B now
    // carries its own enter/hover/exit callbacks writing distinct labels into
    // the same log, so if C stopped blocking, "enterB"/"hoverB" would show up
    // here — this assertion is not satisfied merely by B staying silent for
    // lack of a callback to fire.
    laid.dispatch_pointer_hover(75.0, 75.0);
    assert_eq!(
        log.borrow().as_slice(),
        &["enterA", "enterC", "hoverC", "hoverA"],
        "C, opaque by default, must block B underneath — enterB/hoverB must not appear"
    );
    log.borrow_mut().clear();

    laid.dispatch_pointer_hover(160.0, 160.0);
    assert_eq!(log.borrow().as_slice(), &["exitC", "exitA"]);
}

/// A childless, opaque (default) `MouseRegion` stacked on top of a smaller
/// hit-testable region blocks the pointer everywhere across the whole
/// stack's area — including inside the smaller region's own bounds — because
/// a childless `MouseRegion` sizes to `constraints.biggest()`, not zero.
///
/// Flutter parity: `'an empty opaque MouseRegion is effective'`.
///
/// Red-check: a broken childless-layout branch (returning `Size::ZERO`
/// instead of `constraints.biggest()`) would make this test fail — mutation-
/// run and confirmed (see the build report).
#[test]
fn a_childless_opaque_mouse_region_still_blocks_everything_beneath_it() {
    let bottom_region_hovered = Rc::new(Cell::new(false));
    let (e, h, x) = (
        Rc::clone(&bottom_region_hovered),
        Rc::clone(&bottom_region_hovered),
        Rc::clone(&bottom_region_hovered),
    );

    let laid = harness::pump_widget(
        Stack::new(vec![
            Align::new(Alignment::TOP_LEFT)
                .child(
                    MouseRegion::new()
                        .on_enter(move |_d, _p| e.set(true))
                        .on_hover(move |_d, _p| h.set(true))
                        .on_exit(move |_d, _p| x.set(true))
                        .child(SizedBox::new(10.0, 10.0)),
                )
                .boxed(),
            MouseRegion::new().boxed(),
        ]),
        harness::screen_of(200.0, 200.0),
    );

    // Squarely inside the bottom region's own 10x10 box.
    laid.dispatch_pointer_hover(5.0, 5.0);
    laid.dispatch_pointer_hover(20.0, 20.0);

    assert!(
        !bottom_region_hovered.get(),
        "the empty opaque region on top must swallow the pointer everywhere in the \
         stack, including inside the bottom region's own bounds"
    );
}

// ── Stationary-device postframe recheck ─────────────────────────────────────

/// A region that appears where a stationary pointer already sits is entered
/// on the very next frame, with no new pointer dispatch — Flutter's implicit
/// `MouseTracker.updateAllDevices` postframe recheck
/// (`rendering/mouse_tracker.dart:366`), wired into
/// [`HeadlessBinding::pump_frame`] (`crates/flui-testing/src/lib.rs`) to
/// match the production frame path
/// (`crates/flui-app/src/app/binding.rs`). Enter only, never hover: Flutter's
/// own `_handleDeviceUpdateMouseEvents` (`mouse_tracker.dart:399-430`), the
/// handler this recheck drives, synthesizes only enter/exit events — never a
/// hover — and the upstream oracle test itself asserts `move` (hover) stays
/// null; `MouseRegion.on_hover` fires only from an actual pointer motion.
///
/// Flutter parity: `'triggers pointer enter when widget appears'`.
#[test]
fn stationary_pointer_over_a_newly_appeared_region_triggers_enter_next_frame() {
    let log: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
    let (on_enter, on_hover, on_exit) = (Rc::clone(&log), Rc::clone(&log), Rc::clone(&log));

    let mut laid = harness::pump_widget(
        Align::new(Alignment::CENTER).child(SizedBox::new(100.0, 100.0)),
        harness::screen_of(300.0, 300.0),
    );

    // Register the stationary device at the region's future center -- no
    // region is mounted yet, so nothing fires.
    laid.dispatch_pointer_hover(150.0, 150.0);
    assert!(
        log.borrow().is_empty(),
        "no region is mounted yet, so the hover must find nothing"
    );

    laid.pump_widget(
        Align::new(Alignment::CENTER).child(
            MouseRegion::new()
                .on_enter(move |_d, _p| on_enter.borrow_mut().push("enter"))
                .on_hover(move |_d, _p| on_hover.borrow_mut().push("hover"))
                .on_exit(move |_d, _p| on_exit.borrow_mut().push("exit"))
                .child(SizedBox::new(100.0, 100.0)),
        ),
    );

    assert_eq!(
        log.borrow().as_slice(),
        &["enter"],
        "a region mounted under a stationary pointer must be entered on the \
         very next frame, with no new pointer dispatch -- the frame that \
         mounts it must itself re-hit-test every stationary device; the \
         postframe recheck synthesizes enter/exit only, never hover"
    );
}

/// A region that moves under a stationary pointer (its bounds change via a
/// rebuild, not a new pointer event) is entered on the very next frame.
///
/// Flutter parity: `'triggers pointer enter when widget moves in'`.
#[test]
fn stationary_pointer_over_a_region_that_moves_in_triggers_enter_next_frame() {
    let log: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));

    fn region(log: &Rc<RefCell<Vec<&'static str>>>) -> MouseRegion {
        let (e, h, x) = (Rc::clone(log), Rc::clone(log), Rc::clone(log));
        MouseRegion::new()
            .on_enter(move |_d, _p| e.borrow_mut().push("enter"))
            .on_hover(move |_d, _p| h.borrow_mut().push("hover"))
            .on_exit(move |_d, _p| x.borrow_mut().push("exit"))
            .child(SizedBox::new(100.0, 100.0))
    }

    // Region spans (0,0)-(100,100) at top-left of a 300x300 screen.
    let mut laid = harness::pump_widget(
        Align::new(Alignment::TOP_LEFT).child(region(&log)),
        harness::screen_of(300.0, 300.0),
    );

    // The screen's center, where the region will move to below -- outside
    // the top-left region today, so nothing fires yet.
    laid.dispatch_pointer_hover(150.0, 150.0);
    assert!(
        log.borrow().is_empty(),
        "the stationary point starts outside the top-left region"
    );

    // Same region, moved to center via a rebuild -- no new pointer dispatch.
    laid.pump_widget(Align::new(Alignment::CENTER).child(region(&log)));

    assert_eq!(
        log.borrow().as_slice(),
        &["enter"],
        "a region that moves under a stationary pointer must be entered on \
         the very next frame, enter/exit only, never hover"
    );
}

/// A region that moves out from under a stationary pointer (its bounds
/// change via a rebuild, not a new pointer event) is exited on the very next
/// frame.
///
/// Flutter parity: `'triggers pointer exit when widget moves out'`.
#[test]
fn stationary_pointer_over_a_region_that_moves_out_triggers_exit_next_frame() {
    let log: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));

    fn region(log: &Rc<RefCell<Vec<&'static str>>>) -> MouseRegion {
        let (e, h, x) = (Rc::clone(log), Rc::clone(log), Rc::clone(log));
        MouseRegion::new()
            .on_enter(move |_d, _p| e.borrow_mut().push("enter"))
            .on_hover(move |_d, _p| h.borrow_mut().push("hover"))
            .on_exit(move |_d, _p| x.borrow_mut().push("exit"))
            .child(SizedBox::new(100.0, 100.0))
    }

    // Region spans (100,100)-(200,200), centered in a 300x300 screen.
    let mut laid = harness::pump_widget(
        Align::new(Alignment::CENTER).child(region(&log)),
        harness::screen_of(300.0, 300.0),
    );

    laid.dispatch_pointer_hover(150.0, 150.0);
    assert_eq!(
        log.borrow().as_slice(),
        &["enter", "hover"],
        "an ordinary dispatched hover still fires enter then hover"
    );
    log.borrow_mut().clear();

    // Same region, moved to top-left via a rebuild -- no new pointer
    // dispatch. The stationary pointer at (150,150) is now outside it.
    laid.pump_widget(Align::new(Alignment::TOP_LEFT).child(region(&log)));

    assert_eq!(
        log.borrow().as_slice(),
        &["exit"],
        "a region that moves out from under a stationary pointer must be \
         exited on the very next frame, with no new pointer dispatch"
    );
}

/// A still-mounted, still-hit-testable region rebuilt with an empty callback
/// set must not synthesize a stale exit against the callback set that was
/// active before the rebuild.
///
/// Pins a regression the postframe-recheck wiring above introduced and that
/// the model, not just the recheck, had to correct: the first version of
/// `MouseRegion::sync_mouse_region_target` unregistered the lane target the
/// moment its callback set went empty, which made
/// `RenderMouseRegion::mouse_tracker_annotation`
/// (`crates/flui-objects/src/interaction/mouse_region.rs`) stop contributing
/// an annotation to the very next hit test — the recheck then read that as
/// "the region departed" and fired the OLD `on_exit`, even though the
/// region was never removed from the tree and the pointer never moved.
/// Flutter has no such failure mode: `RenderMouseRegion.onEnter`/`onHover`/
/// `onExit` are plain nullable fields on a render object that stays a valid
/// `MouseTrackerAnnotation` for as long as it is attached
/// (`rendering/proxy_box.dart`) — nulling a field never deregisters
/// anything. `sync_mouse_region_target` now keeps the lane target
/// registered across the rebuild and replaces its callbacks with the empty
/// set instead, matching that.
///
/// No Flutter parity id: no `mouse_region_test.dart` case rebuilds a hovered
/// region down to zero callbacks under a stationary pointer.
#[test]
fn rebuilding_a_hovered_region_down_to_no_callbacks_fires_nothing() {
    let log: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
    let (on_enter, on_exit) = (Rc::clone(&log), Rc::clone(&log));

    let mut laid = harness::pump_widget(
        Align::new(Alignment::CENTER).child(
            MouseRegion::new()
                .on_enter(move |_d, _p| on_enter.borrow_mut().push("enter"))
                .on_exit(move |_d, _p| on_exit.borrow_mut().push("exit"))
                .child(SizedBox::new(100.0, 100.0)),
        ),
        harness::screen_of(300.0, 300.0),
    );

    laid.dispatch_pointer_hover(150.0, 150.0);
    assert_eq!(
        log.borrow().as_slice(),
        &["enter"],
        "an ordinary dispatched hover over the mounted region fires enter"
    );
    log.borrow_mut().clear();

    // Same region, same position, rebuilt with every callback removed --
    // still mounted, still hit-testable, at the SAME stationary pointer
    // position. No new pointer dispatch.
    laid.pump_widget(
        Align::new(Alignment::CENTER).child(MouseRegion::new().child(SizedBox::new(100.0, 100.0))),
    );

    assert_eq!(
        log.borrow().len(),
        0,
        "a still-mounted, still-hit-testable region rebuilt down to no \
         callbacks must not fire a stale exit against the callback set that \
         was active before the rebuild; got {:?}",
        log.borrow()
    );
}

/// The postframe recheck runs INSIDE [`HeadlessBinding::pump_frame`]'s own
/// `UpdateScheduler::drive_frame` pipeline closure, in the same
/// `PersistentCallbacks` slot as layout/paint — not after `drive_frame`
/// returns. A post-frame callback an enter/exit callback queues must
/// therefore land in the SAME frame's post-frame phase, matching production
/// (`AppBinding::render_frame_entered`, `crates/flui-app/src/app/
/// binding.rs`, which calls `MouseTracker::update_all_devices` from inside
/// the same `drive_frame` closure it runs its own layout/paint step in —
/// see `crates/flui-app/src/app/runner.rs`) and the oracle
/// (`_scheduleMouseTrackerUpdate` posts `updateAllDevices` from
/// `_handlePersistentFrameCallback`, still inside the persistent phase,
/// ahead of the post-frame queue). Running the recheck after `drive_frame`
/// returns instead — as an earlier version of
/// [`HeadlessBinding::pump_frame`] did — would defer that queued callback
/// to a LATER pump.
///
/// Goes around [`flui_scheduler::UpdateScheduler::add_post_frame_callback`]
/// directly rather than through a `StatefulView` rebuild handle: driving
/// this same ordering question end-to-end through a real widget rebuild is
/// exactly the "next postframe" cases' missing
/// `StatefulView`-rebuild-handle-in-callback plumbing (see the not-ported
/// section below) — disproportionate cost for this one placement check,
/// which the lower-level scheduler call proves directly.
#[test]
fn stationary_enter_queues_a_post_frame_callback_for_the_same_frame() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    let mut laid = harness::pump_widget(
        Align::new(Alignment::CENTER).child(SizedBox::new(100.0, 100.0)),
        harness::screen_of(300.0, 300.0),
    );
    laid.dispatch_pointer_hover(150.0, 150.0);

    let scheduler = laid.scheduler();
    let post_frame_fires: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
    let post_frame_fires_for_enter = Arc::clone(&post_frame_fires);

    // Region mounted under the stationary pointer above -- entered by the
    // postframe recheck, not by a new pointer dispatch. Its `on_enter`
    // queues a post-frame callback the same way a widget's rebuild handle
    // would from inside a real `setState`.
    laid.pump_widget(
        Align::new(Alignment::CENTER).child(
            MouseRegion::new()
                .on_enter(move |_d, _p| {
                    let post_frame_fires = Arc::clone(&post_frame_fires_for_enter);
                    scheduler.add_post_frame_callback(Box::new(move |_timing| {
                        post_frame_fires.fetch_add(1, Ordering::SeqCst);
                    }));
                })
                .child(SizedBox::new(100.0, 100.0)),
        ),
    );

    assert_eq!(
        post_frame_fires.load(Ordering::SeqCst),
        1,
        "a post-frame callback queued from the postframe recheck's enter \
         callback must fire exactly once, in the SAME pump_widget call that \
         mounted the region -- not zero times (deferred to a later pump) \
         and not more than once"
    );
}

// ── Device kind ──────────────────────────────────────────────────────────

/// A stylus (pen) hover delivers enter/hover/exit exactly like a mouse hover
/// — `MouseTracker::update_with_motion` gates on `Mouse | Pen`
/// (`mouse_tracker.rs:220-222`), matching Flutter's own
/// `mouse`/`stylus`-only gate for enter/exit (`mouse_tracker.dart:302`).
///
/// Flutter parity: `'stylus input works'`.
#[test]
fn stylus_hover_delivers_enter_hover_exit_like_a_mouse() {
    let log: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
    let (on_enter, on_hover, on_exit) = (Rc::clone(&log), Rc::clone(&log), Rc::clone(&log));

    let laid = harness::pump_widget(
        Align::new(Alignment::TOP_LEFT).child(
            MouseRegion::new()
                .on_enter(move |_d, _p| on_enter.borrow_mut().push("enter"))
                .on_hover(move |_d, _p| on_hover.borrow_mut().push("hover"))
                .on_exit(move |_d, _p| on_exit.borrow_mut().push("exit"))
                .child(SizedBox::new(10.0, 10.0)),
        ),
        harness::screen_of(20.0, 20.0),
    );

    laid.dispatch_pointer_hover_with_kind(20.0, 20.0, PointerType::Pen);
    assert!(log.borrow().is_empty(), "starts outside the 10x10 box");

    laid.dispatch_pointer_hover_with_kind(5.0, 5.0, PointerType::Pen);
    assert_eq!(log.borrow().as_slice(), &["enter", "hover"]);
    log.borrow_mut().clear();

    laid.dispatch_pointer_hover_with_kind(20.0, 20.0, PointerType::Pen);
    assert_eq!(log.borrow().as_slice(), &["exit"]);
}

/// A touch-originated hover reaches `on_hover` — exactly once — while
/// `on_enter`/`on_exit` correctly do not fire for touch at all.
///
/// `on_enter`/`on_exit` stay gated to `Mouse | Pen`
/// (`MouseTracker::update_with_motion`, `mouse_tracker.rs:220-222`), matching
/// Flutter's own `MouseTracker.updateWithEvent` gate (`mouse_tracker.dart:302`).
/// `on_hover` has no such gate: it is `RenderMouseRegion.handleEvent`'s
/// concern (`proxy_box.dart`), not the tracker's, and fires for any
/// `PointerHoverEvent` reaching the hit-test target regardless of device
/// kind.
///
/// Flutter parity: `'detects hover from touch devices'`.
#[test]
fn detects_hover_from_touch_devices() {
    let hover_count = Rc::new(Cell::new(0u32));
    let last_hover_position = Rc::new(Cell::new(Offset::ZERO));
    let enters: Rc<RefCell<Vec<Offset>>> = Rc::new(RefCell::new(Vec::new()));
    let exits: Rc<RefCell<Vec<Offset>>> = Rc::new(RefCell::new(Vec::new()));
    let (hover_counter, hover_position) =
        (Rc::clone(&hover_count), Rc::clone(&last_hover_position));
    let on_enter = Rc::clone(&enters);
    let on_exit = Rc::clone(&exits);

    let laid = harness::pump_widget(
        Align::new(Alignment::CENTER).child(
            MouseRegion::new()
                .on_enter(move |_d, p| on_enter.borrow_mut().push(p))
                .on_hover(move |_d, p| {
                    hover_counter.set(hover_counter.get() + 1);
                    hover_position.set(p);
                })
                .on_exit(move |_d, p| on_exit.borrow_mut().push(p))
                .child(SizedBox::new(100.0, 100.0)),
        ),
        harness::screen_of(300.0, 300.0),
    );

    laid.dispatch_pointer_hover_with_kind(150.0, 150.0, PointerType::Touch);

    assert_eq!(
        hover_count.get(),
        1,
        "a touch-originated hover must reach on_hover exactly once"
    );
    assert_eq!(last_hover_position.get(), offset(150.0, 150.0));
    assert!(
        enters.borrow().is_empty(),
        "on_enter stays gated to Mouse | Pen -- touch must not trigger it"
    );
    assert!(
        exits.borrow().is_empty(),
        "on_exit stays gated to Mouse | Pen -- touch must not trigger it"
    );
}

// ── Interleaving with other hit-path targets ────────────────────────────────

/// A `Listener`'s `on_pointer_hover` and a nested `MouseRegion`'s `on_hover`
/// must fire in hit-path order relative to EACH OTHER, not as two separate
/// full passes over the path (every ordinary pointer target, then every
/// mouse region). Flutter delivers both through the SAME single per-entry
/// loop -- `GestureBinding.dispatchEvent`'s `for (final HitTestEntry entry in
/// hitTestResult.path) { entry.target.handleEvent(...); }`
/// (`gestures/binding.dart:496`) -- and `HitTestResult.path` is ordered
/// leaf-first, "the most specific first" (`gestures/hit_test.dart:131-132`).
/// For `Listener(outer) > MouseRegion(inner)` the inner region is the more
/// specific target, so its `on_hover` must run before the outer listener's
/// `on_pointer_hover`.
///
/// Not a Flutter-oracle port (no single upstream test isolates this
/// interleaving); a regression test for FLUI's own two-pass dispatch
/// (all ordinary targets, then all mouse regions) losing that relative order.
///
/// The leaf is a `ColoredBox`, not a bare `SizedBox`: an unpainted `SizedBox`
/// has no `hitTestSelf` override and no child of its own, so it is correctly
/// never part of the hit path (matching Flutter) -- a `DeferToChild` listener
/// wrapping one would never register, which would make this test vacuous
/// rather than red for the right reason.
#[test]
fn hover_fires_leaf_first_through_a_listener_wrapping_a_mouse_region() {
    let log: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
    let (listener_log, region_log) = (Rc::clone(&log), Rc::clone(&log));

    let laid = harness::pump_widget(
        Listener::new()
            .on_pointer_hover(move |_event| listener_log.borrow_mut().push("listener"))
            .child(
                MouseRegion::new()
                    .on_hover(move |_device, _position| region_log.borrow_mut().push("region"))
                    .child(ColoredBox::new(Color::rgb(10, 20, 30))),
            ),
        harness::screen_of(20.0, 20.0),
    );

    laid.dispatch_pointer_hover(10.0, 10.0);

    assert_eq!(
        log.borrow().as_slice(),
        &["region", "listener"],
        "the innermost MouseRegion is the leaf of the hit path and must \
         receive on_hover before the outer Listener's on_pointer_hover"
    );
}

/// As above with the nesting reversed -- pins "leaf-first", not "MouseRegion
/// always fires first". `MouseRegion(outer) > Listener(inner)`: the inner
/// `Listener` is now the more specific target and must fire first.
#[test]
fn hover_fires_leaf_first_through_a_mouse_region_wrapping_a_listener() {
    let log: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
    let (region_log, listener_log) = (Rc::clone(&log), Rc::clone(&log));

    let laid = harness::pump_widget(
        MouseRegion::new()
            .on_hover(move |_device, _position| region_log.borrow_mut().push("region"))
            .child(
                Listener::new()
                    .on_pointer_hover(move |_event| listener_log.borrow_mut().push("listener"))
                    .child(ColoredBox::new(Color::rgb(10, 20, 30))),
            ),
        harness::screen_of(20.0, 20.0),
    );

    laid.dispatch_pointer_hover(10.0, 10.0);

    assert_eq!(
        log.borrow().as_slice(),
        &["listener", "region"],
        "the innermost Listener is the leaf of the hit path and must receive \
         on_pointer_hover before the outer MouseRegion's on_hover"
    );
}
