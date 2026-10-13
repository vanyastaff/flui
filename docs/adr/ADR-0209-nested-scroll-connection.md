# ADR-0209: Nested scrolling is one owner-local connection chain over one offset primitive

- **Status:** Accepted (2026-10-13); implemented by the interaction completeness spec
- **Date:** 2026-10-13
- **Supersedes:** [ADR-0169](ADR-0169-nested-scroll-ballistic-handoff.md)
- **Related:** [ADR-0051](ADR-0051-anchor-stationary-scroll-correction.md),
  [ADR-0157](ADR-0157-native-scroll-range-actions.md),
  [ADR-0112](ADR-0112-gesture-terminal-reasons.md),
  [ADR-0168](ADR-0168-descendant-reveal-through-scroll-ancestors.md),
  [ADR-0127](ADR-0127-exceptional-path-retention.md),
  [ADR-0207](ADR-0207-host-input-boundary-and-timeline.md),
  [ADR-0208](ADR-0208-gesture-ownership-table-and-composition.md) (who owns the contact)

## Context

ADR-0169 hands only a fling's boundary velocity to the nearest same-axis ancestor; drag, wheel,
keyboard, assistive and programmatic offsets take separate paths, so a list in a pager,
pull-to-refresh, a collapsing header and a zoomed photo in a pager cannot share a delta. Its
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
       fn post_fling(&self, step: &FlingStep) -> FlingHandoff { FlingHandoff::Declined }
       fn sequence_ended(&self, sequence: InputSequenceId, reason: ScrollEndReason) {}
   }
   ```

   `pre_scroll` runs root to leaf, `post_scroll` leaf to root; only the dispatcher builds steps
   (`for_test` behind `testing`). There is **no `pre_fling`**: a fling is a stream of `SideEffect`
   deltas (item 4), so a parent takes motion first in `pre_scroll`, and a release decision (refresh
   past threshold) to `sequence_ended(Released)`. A velocity hook would be a second path to the
   same decision; Compose's is `suspend` so the parent animates inside it, which a synchronous frame
   path forbids. `FlingHandoff::{Declined, Accepted}`; `ScrollEndReason::{Released, Cancelled,
   Preempted, ArenaLost}`.
2. **Cause and device.** `ScrollCause::{UserInput, Momentum, SideEffect, Programmatic}`: a
   contact, wheel, key or assistive scroll action; platform inertia (`ScrollPhase::Momentum*`,
   DirectManipulation); framework motion (fling, DnD autoscroll); application and reveal requests.
   `ScrollDevice::{Pointer { kind, device }, Keyboard, Assistive, Programmatic}` reuses
   `PointerKind` and `DeviceId`. Participants decide by these, never by delta size.
3. **One offset primitive.** Every offset change (drag, wheel, keyboard, assistive actions per
   ADR-0157, `reveal(rect)` per ADR-0168, controller jumps and animations, DnD autoscroll, anchor
   correction per ADR-0051) goes through the dispatcher with its cause. `reveal(rect)` passes every
   ancestor through `pre_scroll`/`post_scroll` and keeps ADR-0169's rebasing of the published
   target by the signed current-minus-published pixels, reversal included. A `UserInput` step on a
   surface ends a running `Programmatic` or smooth scroll there with `Preempted`.
4. **Fling.** The leaf runs its ballistic simulation and dispatches each frame's delta as
   `SideEffect` under the releasing `InputSequenceId`. At a hard boundary the residual is
   `ScrollPhysics::boundary_velocity`: none by default; `ClampingScrollPhysics` reads
   `FrictionSimulation::time_at_x` and `dx` with its run's friction and tolerance (no second
   estimator). `post_fling` offers it nearest-first, projected into each owner's orientation; a
   saturated clamping owner declines, a bouncing owner at its outward extent may accept before its
   grandparent, a bouncing leaf keeps its own recovery. `Accepted` starts the acceptor's run under
   the **same** sequence; `sequence_ended` reaches each participant once, after the last run.
   `Momentum` never calls `post_fling` (platform inertia is the fling). A new `UserInput` sequence
   cancels a handed-off run with `Preempted`. Only normal completion of the exact run (same
   generation, live owner) transfers. The measured device pixel ratio captured at release reaches
   every boundary probe and simulation factory of the chain; no policy sees a default ratio.
5. **Leaf and latch.** With a contact, the leaf is the arena owner (ADR-0208). Without one (wheel,
   keyboard, assistive, autoscroll) it is the deepest participant under the point or focus that
   can consume the first delta, by the same code as `AxisLock`. The leaf and chain latch by
   `(InputSurface, DeviceId)` for the sequence; every source has an idle timeout on `EventTime`
   from `GestureSettings::preset`, phased ones longer (Wayland can send Start without End).
6. **Remainder policy.** `OverscrollBehavior::{Auto, Contain}` (CSS `overscroll-behavior`) is a
   channel policy, never an arena input. `Contain` stops the remainder at that scrollable. `Auto`
   follows the preset's per-platform transfer rule: Android hands the remainder to the ancestor
   mid-sequence; iOS and macOS keep the latched scrollable and let the ancestor move on the next
   sequence (to be confirmed on device).
7. **Participant contract.** The chain is snapshotted once per sequence into
   `SmallVec<[Weak<dyn NestedScrollConnection>; 4]>`; a link closed mid-pass is skipped. No borrow
   is held across a participant; a dispatch from inside one is refused with a diagnostic. A
   non-finite, wrong-signed or larger-than-`available` result is clamped with a diagnostic. A
   panicking participant counts as consuming everything, its link closes, it gets
   `sequence_ended(Cancelled)`, and ADR-0127 containment keeps the first failure authoritative.
   Steady-state dispatch allocates nothing. Custom physics runs without widget-state borrows;
   transfer policy is computed before the factory returns its `Box<dyn Simulation>`.
8. **Endpoint ownership.** The endpoint becomes `Rc` with `Cell` state. A change of controller,
   authored physics, axis or parent identity, or unmount, closes it; default physics belongs to
   widget state, so a same-configuration rebuild keeps it. No global registry or label comparison.
9. **Snapping.** `SnapPolicy` is one open trait shaped like `ScrollPhysics`
   (`target(&self, &ScrollMetrics, velocity) -> f64`; `PageSnap`, `AnchorSnap<K>`), held as
   `Rc<dyn SnapPolicy>` and compared by `Rc::ptr_eq`; a non-finite target is clamped with a
   diagnostic. `PageView` is `Scrollable` plus `SnapPolicy`. `AnchoredDrag<K>` is a bounded
   one-dimensional scroll leaf with the same policy and two roles: as a **leaf** (a `SwipeActions`
   row) it is an axial arena member that consumes first and passes the remainder up; as an
   **ancestor** (dismissing a photo) it is only a chain participant, through `connection()`.
10. **Threading seam.** Scrolling is owner-driven. The step, cause, device, end-reason and
    overscroll types and the built-in fling and snap curves stay plain `Send + 'static` data; the
    connection gains no `Send` bound. A future compositor-side scroller hooks into the dispatcher,
    admits only chains without user connections and reports a typed reason for the rest
    ([threading](../research/2026-10-13-interaction-threading-model.md) Q3).

## Consequences and verification

The public `nested_scroll_channel` table in `crates/flui-widgets/tests/contracts.rs`, driven by
`PointerScript` for touch and mouse at least, holds: `list_in_pager_shares_deltas_by_axis`,
`photo_in_pager_hands_pan_remainder_to_pager` (30° and 50° starts, edge at start, second finger,
cancel), `pull_to_refresh_ignores_wheel_and_momentum_and_resets_on_loss`,
`collapsing_header_reveals_focused_row_from_tab_and_assistive_request`,
`fling_handoff_inherits_sequence_across_three_levels`, `momentum_never_offers_post_fling`,
`new_touch_preempts_handed_off_fling`, `contactless_leaf_is_deepest_consuming_participant`,
`latch_expires_after_idle_timeout_for_every_source`, `user_input_preempts_programmatic_scroll`,
`participant_result_is_clamped`, `reentrant_dispatch_is_refused` and
`participant_panic_consumes_all_and_next_sequence_recovers` (one panic, two competing, after
containment). ADR-0169's `nested_fling_*` and `show_on_screen_*` witnesses stay as rows;
`snap_policy_targets` covers `PageView` and `AnchoredDrag` together. A counting-allocator target
pins allocation; `assert_impl_all!` pins the `Send` data; a `compile_pass` fixture accepts an
`Rc`-capturing connection. Linux CI runs all of these headless. DirectManipulation inertia, macOS
momentum, Wayland's unterminated phases and the iOS transfer rule run only on native hosts.

## Alternatives rejected

- **Declarative consumption orders** (followups §5.3): refresh and collapsing headers need
  state-dependent order. **Whole-event handling** (Masonry `Portal`, GPUI; §5.1): no partial delta.
- **Compositor-thread scrolling now** (Chromium cc, Firefox APZ; threading Q3): a synchronous
  return value is a scroll-linked effect, which those engines cannot run off the main thread.
