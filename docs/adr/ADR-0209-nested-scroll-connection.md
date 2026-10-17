# ADR-0209: Nested scrolling is one owner-local connection chain over one offset primitive

- **Status:** Accepted (2026-10-13). The channel lives in `flui-widgets` `scroll::nested`, and
  `Scrollable` is its leaf and participant; `PageView`, `AnchoredDrag`, pull-to-refresh, the
  collapsing header and `InteractiveViewer` move onto it with their
  [tasks](../plans/specs/flui-interaction/completeness/tasks.md).
- **Date:** 2026-10-13
- **Amended:** 2026-10-17 — §8: a new physics value no longer closes the endpoint (it is taken
  in place, as a snap policy is), and a change that does close it on the same position hands the
  contact, the run and the sequences in progress to the successor.
- **Supersedes:** [ADR-0169](ADR-0169-nested-scroll-ballistic-handoff.md)
- **Related:** [ADR-0051](ADR-0051-anchor-stationary-scroll-correction.md),
  [ADR-0157](ADR-0157-native-scroll-range-actions.md),
  [ADR-0112](ADR-0112-gesture-terminal-reasons.md),
  [ADR-0168](ADR-0168-descendant-reveal-through-scroll-ancestors.md),
  [ADR-0127](ADR-0127-exceptional-path-retention.md),
  [ADR-0207](ADR-0207-host-input-boundary-and-timeline.md),
  [ADR-0208](ADR-0208-gesture-ownership-table-and-composition.md)

## Context

ADR-0169 hands only a fling's boundary velocity to the nearest same-axis ancestor; other offsets
take separate paths, so a list in a pager, pull-to-refresh, a collapsing header and a zoomed photo
in a pager cannot share a delta. Its
`Arc<FlingEndpoint>` with an `AtomicBool` rests on "the Send completion callback", which is wrong:
`AnimationRunFuture::when_complete_or_cancel` takes `FnMut(..) + 'static` without `Send`
(`crates/flui-animation/src/run_future.rs`). The arena owns contacts (ADR-0208); continuous deltas
need their own channel ([followups](../research/2026-10-12-interaction-input-followups.md) §5).

## Decision

1. **The connection.** In `flui-widgets` (`scroll::nested`), driven by one concrete
   `NestedScrollDispatcher` ([spec](../plans/specs/flui-interaction/completeness/design.md) §5):

   ```rust
   #[non_exhaustive] pub struct ScrollStep { pub sequence: InputSequenceId, pub cause: ScrollCause,
                                             pub device: ScrollDevice, pub available: Vec2 }
   #[non_exhaustive] pub struct FlingStep { pub sequence: InputSequenceId, pub device: ScrollDevice,
                                            pub available: Velocity }
   pub trait NestedScrollConnection {                    // object-safe, &self, !Send by design
       fn sequence_started(&self, sequence: InputSequenceId) {}
       fn pre_scroll(&self, step: &ScrollStep) -> Vec2 { Vec2::ZERO }
       fn post_scroll(&self, step: &ScrollStep, consumed_by_child: Vec2) -> Vec2 { Vec2::ZERO }
       fn released(&self, step: &FlingStep) {}
       fn post_fling(&self, step: &FlingStep) -> FlingHandoff { FlingHandoff::Declined }
       fn sequence_ended(&self, sequence: InputSequenceId, reason: ScrollEndReason) {}
   }
   ```

   `pre_scroll` runs root to leaf, `post_scroll` leaf to root; only the dispatcher builds steps
   (`for_test` behind `testing`). `released` is called once when the sequence's last user contact
   lifts, before any fling run; release decisions (refresh past threshold, snap) use it, and
   `sequence_ended` arrives after the last run. There is **no `pre_fling`**: a fling is a stream of
   `SideEffect` deltas (item 4) a parent can take first in `pre_scroll`; Compose's hook is
   `suspend` so the parent animates inside it, which a synchronous frame path forbids. Also
   `ScrollEndReason::{Released, Cancelled, Preempted, ArenaLost}`, `FlingHandoff::{Declined, Accepted}`.
2. **Cause and device.** `ScrollCause::{UserInput, Momentum, SideEffect, Programmatic}`: a
   contact, wheel, key or assistive scroll action; platform inertia (`ScrollPhase::Momentum*`,
   DirectManipulation); framework motion (fling, DnD autoscroll); application and reveal requests.
   `ScrollDevice::{Pointer { kind, device }, Keyboard, Assistive, Programmatic}` reuses
   `PointerKind` and `DeviceId`. `ScrollCause`, `ScrollDevice` and `ScrollEndReason` are
   `#[non_exhaustive]` (Surface Dial, Digital Crown); `FlingHandoff` is exhaustive — a two-way
   answer whose growth would be a contract change. Participants decide by these, never by delta size, and opt in to `Programmatic`: the
   default methods ignore it (Compose deprecated `Relocate` for unintended consumption).
3. **One offset primitive.** Every scroll (drag, wheel, keyboard, assistive actions per ADR-0157,
   `reveal(rect)` per ADR-0168, controller jumps and animations, DnD autoscroll) goes through the
   dispatcher with its cause. `reveal(rect)` passes every ancestor through the chain and keeps
   ADR-0169's rebasing of the published target by the signed current-minus-published pixels. A
   `UserInput` step on a surface ends a running `Programmatic` or smooth scroll with `Preempted`.
   Anchor correction (ADR-0051) is not a scroll: the viewport applies it inside layout without
   dispatch, no participant sees it, and it neither ends nor latches a sequence.
4. **Fling.** The leaf runs its ballistic simulation and dispatches each frame's delta as
   `SideEffect` under the releasing `InputSequenceId`; the run is authoritative over its own leaf (a
   bouncing run overscrolls on purpose). A physics transfers when `ScrollPhysics::boundary_velocity`
   returns the velocity its simulation keeps where it reaches the extent it heads for (none by
   default); that run rests on the extent, and the residual — the simulation's `dx` at the
   crossing — is offered through `post_fling` nearest-first and projected into each owner's
   orientation. This is the unbounded run's rule, evaluated at the extent: the two runs deliver
   the same deltas up to the crossing, and the first tick where the unbounded run's leaf and
   chain would consume less than the step is the crossing tick, whose residual is the
   simulation's `dx` there — the value `boundary_velocity` returns. They differ in when it is
   offered: at the frame the bounded run completes, not mid-frame. Participants may take part
   of the run's steps (`pre_scroll`); the leaf then lags the run's own value by exactly what they
   took, and the run has reached the extent when the leaf's pixels plus that lag equal it, so a
   participant taking part of a fling does not swallow its residual. A saturated clamping owner declines, a bouncing owner at its
   outward extent may accept before its grandparent, a bouncing leaf keeps its own recovery.
   `Accepted` starts the acceptor's run under the **same** sequence; `sequence_ended` reaches each
   participant once, after the last run. `Momentum` never calls `post_fling` (platform inertia is
   the fling). A new `UserInput` sequence cancels a handed-off run with `Preempted`; a cancelled or
   superseded run never transfers. The device pixel ratio measured at release reaches every
   boundary probe and simulation factory of the chain; no policy sees a default ratio.
5. **Leaf and latch.** The chain latches, per direction, to the deepest participant that consumes
   a non-zero part of the sequence's first non-zero delta (the arena owner included; an owner at
   its edge does not latch); contactless sources start under the point or focus, by the same code
   as `AxisLock`. A keyboard sequence is one key press: the focused scrollable (or the one holding
   the focus in its subtree) takes arrows along its axis, PageUp/PageDown and Space/Shift+Space,
   Home and End, from the keyboard device; the key's auto-repeats continue the sequence, its release
   or the next press ends it, and a scrollable that cannot move lets the key bubble to the enclosing
   one. A contact sequence latches until the contact ends; leaf and chain stay latched
   for the lifetime of the `InputSequenceId` the binding minted. The idle timeout that ends a
   contactless sequence (wheel, keyboard, assistive, autoscroll) is the binding's deadline on
   `EventTime` from the preset, phased sources longer (Wayland can send Start without End). The
   channel keeps no timer and never re-derives sequence boundaries; the only re-latch inside a
   sequence is the preset-selected edge exception in §6.
6. **Remainder policy.** `OverscrollBehavior::{Auto, Contain}` (CSS `overscroll-behavior`) is a
   channel policy, never an arena input. `Contain` stops the remainder at that scrollable. Under
   `Auto`, the latch holds for the whole `InputSequenceId` by default (§5); the ancestor moves on
   the next sequence. One preset-selected exception: where the platform preset says the remainder
   transfers mid-drag (the Android behaviour, to be confirmed on device before the preset value
   is fixed), a latched leaf that reports zero consumption at its edge re-latches the chain to the
   nearest ancestor that consumes, within the same `InputSequenceId`; it never re-latches back.
   Row: `mid_drag_transfer_relatches_only_at_edge_within_the_sequence`.
7. **Participant contract.** The chain is snapshotted once per sequence into four inline links,
   each a `Weak<dyn NestedScrollConnection>` or an enclosing scrollable's owner; a link closed
   mid-pass is skipped. No borrow
   is held across a participant; a dispatch from inside one is refused with a diagnostic. A
   non-finite, wrong-signed or larger-than-`available` result is clamped with a diagnostic. A
   panicking participant counts as consuming everything, its link closes, it gets
   `sequence_ended(Cancelled)`, and ADR-0127 containment keeps the first failure authoritative.
   Steady-state dispatch allocates nothing. Custom physics runs without widget-state borrows;
   transfer policy is computed before the factory returns its `Box<dyn Simulation>`.
8. **Endpoint ownership.** The endpoint becomes `Rc` with `Cell` state. Unmount or a change of
   controller, axis, orientation, overscroll behavior or parent identity closes it. A new
   physics value does not: the endpoint takes it in place for the next step and release, and a
   run in progress keeps its simulation, so a build that allocates its physics every time keeps
   its endpoint and its descendants' chains. Closing it on the same position hands what it
   accepted — the contact, the run and the motion it drives, a held key, a drop autoscroll — to
   the successor under the sequences they began, each with the chain it snapshotted; nothing
   halts, and no participant is called. No global registry, no label comparison.
9. **Snapping.** `SnapPolicy: Send + Sync + Debug` is one open trait shaped like `ScrollPhysics`
   (`target(&self, &ScrollMetrics, velocity) -> f64`; `PageSnap` and `AnchorSnap`), held as
   `Arc<dyn SnapPolicy>` like `SharedScrollPhysics`; a non-finite target is clamped with a
   diagnostic. Unlike the physics, a new policy does not close the endpoint (§8): the owner takes
   it in place, it decides the next release, and a run in progress keeps the target it started
   for. `AnchorSnap` holds offsets, not keys: a target is an
   offset, so `DraggableAnchors<K>` keeps the key-to-offset map and hands its offsets to an
   `AnchorSnap`. A release faster than the policy's velocity tolerance takes the next target in
   its direction, also from a target's own offset; within half a device pixel of a target counts
   as on it. `PageView` is `Scrollable` plus `SnapPolicy`.
   `AnchoredDrag<K>` is a bounded one-dimensional scroll leaf with the same policy and two roles:
   as a **leaf** (a `SwipeActions` row) it is an axial arena member that consumes first and passes
   the remainder up; as an **ancestor** (dismissing a photo, a sheet over a list) it is only a
   chain participant: it takes `UserInput` remainders on its axis, and the inner sequence's
   `released` (or its end, without one) settles it under the same sequence id; a fling residual
   it is not already settling for starts that settle. Arriving at another anchor than the current
   one, except programmatically, plays a frame-aligned selection haptic (ADR-0210 §10).
10. **Threading seam.** Scrolling is owner-driven. The step, cause, device, end-reason, overscroll
    and snap types and the built-in fling curves stay `Send` data; the connection gains no `Send`
    bound. A future compositor-side scroller hooks into the dispatcher, admits only chains without
    user connections and reports a typed reason for the rest
    ([threading](../research/2026-10-13-interaction-threading-model.md) Q3).

## Consequences and verification

The `nested_scroll_channel` table in `crates/flui-widgets/tests/contracts.rs`, driven by
`PointerScript` for touch and mouse at least, holds `list_in_pager_shares_deltas_by_axis`,
`photo_in_pager_hands_pan_remainder_to_pager` (30° and 50° starts, edge at start, second finger,
cancel), `pull_to_refresh_ignores_wheel_and_momentum_and_resets_on_loss`,
`collapsing_header_reveals_focused_row_from_tab_and_assistive_request`,
`fling_handoff_inherits_sequence_across_three_levels`, `momentum_never_offers_post_fling`,
`new_touch_preempts_handed_off_fling`, `edge_at_start_latches_ancestor_on_every_preset`,
`channel_latch_ends_exactly_when_binding_mints_a_new_sequence`,
`anchor_correction_bypasses_the_channel`, `user_input_preempts_programmatic_scroll`,
`participant_result_is_clamped`, `reentrant_dispatch_is_refused` and
`participant_panic_consumes_all_and_next_sequence_recovers` (one panic, two competing, after
containment). ADR-0169's `nested_fling_*` and `show_on_screen_*` witnesses stay as rows;
`snap_policy_targets` covers `PageSnap` and `AnchorSnap` on a `Scrollable`, and
`AnchoredDrag`'s `anchored_drag_*` rows join the same table (release and fling, RTL, degenerate
anchor sets, anchors replaced mid-drag, unmount mid-drag, assistive and key steps, haptics, leaf,
participant, and no drop autoscroll); `tests/page_view.rs` covers `PageView`. A counting-allocator target pins
allocation, `assert_impl_all!` the `Send` data, and a `compile_pass` fixture accepts an
`Rc`-capturing connection. Linux CI runs all of these headless; DirectManipulation inertia, macOS
momentum, Wayland's unterminated phases and the iOS latch rule run only on native hosts.

## Alternatives rejected

- **Declarative consumption orders** (followups §5.3): refresh and collapsing headers need
  state-dependent order. **Whole-event handling** (Masonry `Portal`, GPUI; §5.1): no partial delta.
- **Compositor-thread scrolling now** (Chromium cc, Firefox APZ; threading Q3): a synchronous
  return value is a scroll-linked effect, which those engines cannot run off the main thread.
