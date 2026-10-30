# ADR-0209: Nested scrolling is one owner-local connection chain over one offset primitive

- **Status:** Accepted (2026-10-13). The channel lives in `flui-widgets` `scroll::nested`, and
  `Scrollable` is its leaf and participant; `PageView`, `AnchoredDrag`, pull-to-refresh, the
  collapsing header and `InteractiveViewer` move onto it with their
  [tasks](../plans/specs/flui-interaction/completeness/tasks.md).
- **Date:** 2026-10-13
- **Amended:** 2026-10-17 — §8: a new physics value no longer closes the endpoint (it is taken
  in place, as a snap policy is), and a change that does close it on the same position hands the
  contact, the run and the sequences in progress to the successor. 2026-10-17 — §3 and §7: a
  programmatic request made during a pass waits for the pass instead of being refused
  ([research](https://github.com/vanyastaff/flui/blob/083b0f75da003030e908d107825e9f4db4e54437/docs/research/2026-10-17-scroll-jump-during-dispatch.md)); §6a: a remainder catches
  an ancestor's own motion, and the sequence settles every scrollable it moved
  ([research](https://github.com/vanyastaff/flui/blob/083b0f75da003030e908d107825e9f4db4e54437/docs/research/2026-10-17-nested-scroll-spring-remainder.md)). 2026-10-18 — §1:
  a step says whether a contact drives it; §5: a leaf at its extent still carries a wheel or
  trackpad packet to the connections below the nearest enclosing scrollable. 2026-10-18 — §3:
  a reveal moves each scroll ancestor by its own geometry in a programmatic sequence of its
  own, not as one sequence handing a remainder up; §4: new input on a leaf stops the run an
  ancestor drives for the fling the leaf handed it; §5: a contactless source finds its leaf by
  the hit path's event propagation, not by `AxisLock`'s code. 2026-10-19 — §1: steps carry
  their focal point and device pixel ratio, each participant is offered them in its own
  coordinate plane, a participant opts in to programmatic sequences with
  `accepts_programmatic`, and `FlingHandoff::Accepted` carries no future; §5: a wheel or
  trackpad sequence ends at its latch's idle deadline, without a further packet.
  2026-10-26 — §1: the per-axis latch is kept in the leaf's plane, not the presentation's, so a
  motion along one of the leaf's axes latches that axis alone under a rotated subtree; §5: the
  idle deadline moves to the arena a packet is dispatched on when the presentation swaps it.
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
need their own channel ([followups](https://github.com/vanyastaff/flui/blob/083b0f75da003030e908d107825e9f4db4e54437/docs/research/2026-10-12-interaction-input-followups.md) §5).

## Decision

1. **The connection.** In `flui-widgets` (`scroll::nested`), driven by one concrete
   `NestedScrollDispatcher` ([spec](../plans/specs/flui-interaction/completeness/design.md) §5):

   ```rust
   #[non_exhaustive] pub struct ScrollStep { pub sequence: InputSequenceId, pub cause: ScrollCause,
                                             pub device: ScrollDevice, pub contact: bool,
                                             pub available: Vec2, pub focal_point: Point,
                                             pub device_pixel_ratio: f64 }
   #[non_exhaustive] pub struct FlingStep { pub sequence: InputSequenceId, pub device: ScrollDevice,
                                            pub available: Velocity, pub focal_point: Point,
                                            pub device_pixel_ratio: f64 }
   pub trait NestedScrollConnection {                    // object-safe, &self, !Send by design
       fn accepts_programmatic(&self) -> bool { false }
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

   **`Accepted` carries no future.** An answer that handed the channel the acceptor's run
   as an `AnimationRunFuture` would make the channel await a run: an `async` value on the
   frame path, which the framework forbids, and a second completion path beside the
   acceptor's own. The acceptor runs the residual itself: a scrollable on its own
   controller under the same sequence id, whose completion the channel already observes
   like any run's, so the sequence ends after it (§4); a connection's motion is its own,
   and the sequence ends for everyone once no scrollable runs for it, a connection's
   coast included.

   **Focal point and pixel ratio.** `focal_point` is where the input points: the contact,
   or a wheel packet's pointer; input without a position (a key, an assistive action, a
   programmatic scroll) points at the middle of the leaf's box. `device_pixel_ratio` is the
   presentation's at the sequence's latest user input. A fling or a settle keeps its
   release's point and ratio, so a run's steps and its `FlingStep`s match the release that
   started it.

   **Each participant works in its own plane.** A participant's plane is the local space of
   the box it wraps (a `NestedScroll`'s child, a `Scrollable`'s viewport, an
   `InteractiveViewer`'s box outside its own transform), read from the last complete
   layout (`PipelineOwner::transform_to` the root) at each step, the last sample standing
   while a frame holds the layout. The channel carries what is left of a step in the
   presentation's plane and converts it at each participant: `available`, `focal_point` and
   `consumed_by_child` into its plane before it is asked, what it took back out after.
   Under an affine transform a delta and a velocity map through the linear part; under a
   projection a delta is the chord the focal point travels and a velocity maps through the
   projection's tangent at the focal point, not as a one-second chord. A conversion's
   rounding below 10⁻¹² of the step reads as zero, so a quarter turn keeps a delta on one
   axis. The per-axis latch (§5) is kept in the leaf's plane: a remainder crosses into it to
   be masked per axis and back, so a motion along one of the leaf's axes latches that axis
   alone however the leaf is turned, and a leaf asks the latch about its own axes directly. A plane that cannot be
   crossed (a singular or non-finite transform, a focal point behind the camera) offers its
   participant nothing, and a runner whose plane maps no way back (a transform that
   squashes an axis) takes its step alone: no participant is offered a value that is not
   finite, and nothing is published from one. Rows:
   `steps_carry_the_focal_point_and_the_device_pixel_ratio` and
   `each_participant_is_offered_the_step_in_its_own_plane` (a scale about the centre, a
   quarter turn, a squashed axis).
2. **Cause and device.** `ScrollCause::{UserInput, Momentum, SideEffect, Programmatic}`: a
   contact, wheel, key or assistive scroll action; platform inertia (`ScrollPhase::Momentum*`,
   DirectManipulation); framework motion (fling, DnD autoscroll); application and reveal requests.
   `ScrollDevice::{Pointer { kind, device }, Keyboard, Assistive, Programmatic}` reuses
   `PointerKind` and `DeviceId`. `ScrollCause`, `ScrollDevice` and `ScrollEndReason` are
   `#[non_exhaustive]` (Surface Dial, Digital Crown); `FlingHandoff` is exhaustive — a two-way
   answer whose growth would be a contract change. Participants decide by these, never by
   delta size, and opt in to programmatic sequences explicitly (Compose deprecated
   `Relocate` for unintended consumption): `accepts_programmatic` is asked once, when a
   sequence of `ScrollDevice::Programmatic` (a jump, an animation, a reveal, with the settle
   that follows) starts, and a participant that declines — the default, or one that panics
   while it answers — hears nothing of it: no start, no step, no end. A scrollable link
   never takes part; it moves for such a request in a sequence of its own (§3). Row:
   `a_participant_declining_programmatic_scrolls_hears_none_of_them`.
   `ScrollStep::contact` says whether a user contact (a finger, a pen, a mouse with a button
   held) drives the sequence: a mouse drag and a mouse wheel share `PointerKind::Mouse`, and a
   participant that only a dragging hand may move (pull-to-refresh) needs the difference.
3. **One offset primitive.** Every scroll (drag, wheel, keyboard, assistive actions per ADR-0157,
   `reveal(rect)` per ADR-0168, controller jumps and animations, DnD autoscroll) goes through the
   dispatcher with its cause. `reveal(rect)` keeps ADR-0169's rebasing of the published target
   by the signed current-minus-published pixels and moves every scroll ancestor nearest first,
   each by its own geometry (the target, or the visible part of the viewport the previous one
   moved, shifted by that move) as a `Programmatic` sequence of its own through that
   ancestor's chain, so every connection above an ancestor is offered that ancestor's step.
   It is not one sequence whose remainder goes up through `post_scroll`: what an ancestor must
   move to show the inner viewport is not what the inner scrollable could not move (an inner
   list that shows its target leaves no remainder while its own viewport is still off screen,
   and an inner viewport taller than the outer one asks less of it than the remainder would),
   and a remainder is the channel's answer to one user delta, which a reveal is not. A
   participant that wants to answer a reveal (a collapsing header expanding for a row it
   clips) does so as a reveal scope, not as a consumer of programmatic remainders. Row:
   `a_nested_reveal_moves_each_ancestor_in_a_sequence_of_its_own`. A
   `UserInput` step on a surface ends a running `Programmatic` or smooth scroll with `Preempted`.
   Anchor correction (ADR-0051) is not a scroll: the viewport applies it inside layout without
   dispatch, no participant sees it, and it neither ends nor latches a sequence.

   **Programmatic requests during a pass.** A pass is one dispatch through a chain, from entry
   to return (a step, a hand-off, a release, a start or an end). A `ScrollController::jump_to`,
   `animate_to` or reveal requested while a pass runs on the requesting scrollable's chain (from
   a participant, a position listener or a motion hook) is not refused: it is recorded in the
   controller's one command slot, where the latest request replaces any not yet applied, and it
   is applied when the outermost pass on that scrollable returns, before the dispatch that ran it
   returns. The target is clamped to the extents then. A request made while that application
   runs is applied by the same loop, up to a fixed bound (8); past it, the last target waits for
   the next notification with a diagnostic and is never dropped. A panic in the pass does not
   drop the request: it is applied within the pass's containment, and the first failure stays
   authoritative (ADR-0127). If the owner closes on the same position, the successor applies it
   (§8). With no owner, the controller writes the clamped position, also when a jump still waits
   as the scrollable lets go of the controller. A target that is not finite is dropped where it
   is asked for.

   **What a jump cancels and reports.** When a jump is applied, it ends the run the owner drives
   (a fling, a settle, an animation, an accepted residual) `Preempted`, and that run offers no
   residual. It never ends a contact or key still held or a drop autoscroll; those continue from
   the new offset under their own sequence and latch. The jump is a sequence of its own: a
   freshly minted `InputSequenceId`, `ScrollCause::Programmatic`, `ScrollDevice::Programmatic`,
   `sequence_started`, one step, no `released`, `sequence_ended(Released)`. A preempted
   sequence's `sequence_ended` comes before the jump's `sequence_started`. Rows:
   `a_jump_from_a_participant_lands_when_the_pass_returns`,
   `a_jump_from_a_position_listener_never_nests_inside_a_step`,
   `requests_made_in_one_pass_keep_the_latest`,
   `a_landing_jump_preempts_the_run_and_keeps_live_input`,
   `a_jump_asked_before_a_participant_panics_still_lands`,
   `a_jump_feedback_loop_is_bounded_and_keeps_its_target` and
   `a_jump_pending_across_an_unmount_writes_the_position`.
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
   its edge does not latch). A contactless source starts at a leaf found without a contact: a
   wheel or trackpad packet at the deepest scrollable on the hit path that can act on it (each
   `Scrollable` passes on a packet it cannot move by, leaf first), a key at the focused
   scrollable; from that leaf on, the sequence latches by the rule above, in the same
   `ScrollSequence` a contact uses. This is not `AxisLock`'s code: `AxisLock` rails a pan's
   motion onto an axis inside a recognizer that already owns a contact, while a contactless
   source has no contact, no slop and no path to rail, and the event propagation of the hit path
   already names the deepest scrollable that can take it. Rows:
   `keyboard_bubbles_to_the_enclosing_scrollable_and_follows_the_axis` and
   `plain_wheel_pans_the_viewer_then_leaves_its_edge_to_the_list`. A keyboard sequence is one key press: the focused scrollable (or the one holding
   the focus in its subtree) takes arrows along its axis, PageUp/PageDown and Space/Shift+Space,
   Home and End, from the keyboard device; the key's auto-repeats continue the sequence, its release
   or the next press ends it, and a scrollable that cannot move lets the key bubble to the enclosing
   one. A contact sequence latches until the contact ends; leaf and chain stay latched
   for the lifetime of the `InputSequenceId` the binding minted. A key sequence ends with its
   key's release, an assistive action and a drop autoscroll with their own input; the
   channel never re-derives sequence boundaries, and the only re-latch inside a sequence is
   the preset-selected edge exception in §6.

   **A wheel sequence ends at its latch's idle deadline.** The binding latches a wheel or
   trackpad source's packets into one sequence until the source idles past the preset's
   time: `contactless_latch_idle`, 1500 ms, for a phase-less wheel, and
   `phased_latch_idle`, 3000 ms, for a phased source, which may begin without ever ending
   (Wayland). 1500 ms is Firefox's `mousewheel.transaction.timeout`: long enough that the
   notches of one flick of the wheel, and a pause to read between them, stay one sequence,
   short enough that the next gesture a moment later is a new one. The channel takes the
   binding's value rather than a time of its own: a sequence that ended earlier than its
   latch would see its next packet arrive under the same id, and one that ended later would
   keep a latch the binding already let go. With each packet the binding reports when its
   latch ends (`GestureArena::dispatched_latch_end`): at an event time, the idle time after
   the packet, or with this packet, for an end phase. The leaf that leads the sequence
   follows the report once it has handled the packet, moved or not: it keeps one deadline
   registration on the arena's owner clock (`GestureArena::register_deadline`, a source that
   joins no contest), whose time the host's wake already folds in (`next_wake`), and moves
   it to each packet's time; the frame path stays synchronous and the poll runs at the
   owner's deadline tick. When the deadline comes, or after a packet that ends the latch,
   the sequence takes no further input and ends `Released` once its last run (a notch's eased
   motion) comes to rest. The deadline is disarmed before anyone is told and withdrawn when
   the sequence ends any other way, so each participant hears the end once: a participant
   that panics in `sequence_ended` leaves the others theirs, and a leaf removed first ends
   the sequence `Cancelled` and lets the deadline go. Input that still names an ended
   sequence's id is no new sequence: it moves the leaf alone. Row:
   `an_idle_wheel_sequence_ends_at_its_latch_deadline` (idle end, extension by a packet,
   single delivery, a repeated sequence, a panicking participant, removal, and a phased pan
   ending with its end phase).

   **A leaf at its extent still carries a wheel packet.** The binding routes a wheel or trackpad
   packet to the first listener on the hit path that claims it, leaf first, and a scrollable
   claims only a packet it can act on. A user connection has no listener of its own, so a
   scrollable that cannot move by a packet still dispatches it, taking nothing itself, when a
   connection sits between it and the nearest enclosing scrollable, and claims the packet if the
   chain took part of it. The dispatch is an ordinary step under the sequence's latch:
   `pre_scroll` is offered regardless of it, as for a drag at the edge, and `post_scroll` only
   where §5 and §6 admit a remainder. So a header that expands after its content expands on the
   next wheel sequence once the list is at its top, as it does on the next drag, and an
   enclosing scrollable still decides by its own claim when no connection is in between. Rows:
   `collapsing_header_expands_first_or_last` and `momentum_never_offers_post_fling`.
6. **Remainder policy.** `OverscrollBehavior::{Auto, Contain}` (CSS `overscroll-behavior`) is a
   channel policy, never an arena input. `Contain` stops the remainder at that scrollable. Under
   `Auto`, the latch holds for the whole `InputSequenceId` by default (§5); the ancestor moves on
   the next sequence. One preset-selected exception: where the platform preset says the remainder
   transfers mid-drag (the Android behaviour, to be confirmed on device before the preset value
   is fixed), a latched leaf that reports zero consumption at its edge re-latches the chain to the
   nearest ancestor that consumes, within the same `InputSequenceId`; it never re-latches back.
   Row: `mid_drag_transfer_relatches_only_at_edge_within_the_sequence`.

   **6a. A remainder catches; the sequence settles what it moved.** A scrollable that takes a
   non-zero `UserInput` or `Momentum` remainder first stops the run it drives (a settle spring, a
   fling, a snap), whose sequence ends `Preempted`. It then moves only by what it takes, from past
   an extent only toward its range. A remainder it cannot take leaves its run alone, and while it
   runs it takes no `SideEffect` remainder. Every scrollable a sequence moved through the channel
   is owed a settle by that sequence, recorded in the snapshot before the scrollable publishes.
   At `released`, each one settles under the same sequence id, with the release velocity on the
   axes whose latch it holds and from rest on the others, and the leaf runs motion only on the
   axes no ancestor holds. A sequence that ends without a release settles them from rest before
   `sequence_ended`, whatever the reason (cancelled, arena lost, preempted, the leaf unmounted, a
   wheel or key sequence ending). The debt survives a participant panic and resolves through the
   chain node's current owner, so it follows the owner's retirement to its successor on the same
   position. A scrollable with a live contact of its own is settled by that contact. A new contact
   catches a moving ancestor by the hold on Down (ADR-0208 rule 4); when the hold yields to a
   descendant's drag, the ancestor settles from rest and the first remainder catches it. A
   participant connection still settles itself from `released` (§9's `AnchoredDrag` ancestor).
   The iOS preset's behaviour for this case waits on a device recording. Rows:
   `a_remainder_catches_a_springing_ancestor_and_moves_it_once`,
   `a_caught_ancestor_settles_at_release_with_the_latched_velocity` and
   `a_caught_ancestor_settles_when_the_sequence_ends_without_release`.
7. **Participant contract.** The chain is snapshotted once per sequence into four inline links,
   each a `Weak<dyn NestedScrollConnection>` or an enclosing scrollable's owner; a link closed
   mid-pass is skipped. No borrow
   is held across a participant. A programmatic request from inside one waits for the pass (§3);
   user input (a drag, wheel or key step) dispatched from inside one into its own chain is refused
   with a diagnostic. A
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
9. **Snapping.** `SnapPolicy: Debug` is one open, owner-local trait shaped like `ScrollPhysics`
   (`target(&self, &ScrollMetrics, velocity) -> f64`; `PageSnap` and `AnchorSnap`), held as
   `Rc<dyn SnapPolicy>` like `SharedScrollPhysics`; a non-finite target is clamped with a
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
    ([threading](https://github.com/vanyastaff/flui/blob/083b0f75da003030e908d107825e9f4db4e54437/docs/research/2026-10-13-interaction-threading-model.md) Q3).

## Consequences and verification

The `nested_scroll_channel` table in `crates/flui-widgets/tests/contracts.rs`, driven by
`PointerScript` for touch and mouse at least, holds `list_in_pager_shares_deltas_by_axis`,
`photo_in_pager_hands_pan_remainder_to_pager` (30° and 50° starts, edge at start, second finger,
cancel), `pull_to_refresh_ignores_wheel_and_momentum_and_resets_on_loss`,
`collapsing_header_reveals_focused_row_from_tab_and_assistive_request`,
`fling_handoff_inherits_sequence_across_three_levels`, `momentum_never_offers_post_fling`,
`new_touch_preempts_handed_off_fling`, `new_wheel_or_assistive_scroll_preempts_handed_off_fling`,
`edge_at_start_latches_ancestor_on_every_preset`,
`channel_latch_ends_exactly_when_binding_mints_a_new_sequence`,
`anchor_correction_bypasses_the_channel`, `user_input_preempts_programmatic_scroll`,
`participant_result_is_clamped`, `a_participant_cannot_dispatch_input_into_its_own_chain` and
`participant_panic_consumes_all_and_next_sequence_recovers` (one panic, two competing, after
containment), with `steps_carry_the_focal_point_and_the_device_pixel_ratio`,
`each_participant_is_offered_the_step_in_its_own_plane`,
`a_participant_declining_programmatic_scrolls_hears_none_of_them` and
`an_idle_wheel_sequence_ends_at_its_latch_deadline`. ADR-0169's `nested_fling_*` and `show_on_screen_*` witnesses stay as rows;
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
