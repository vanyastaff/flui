# ADR-0169: Nested scroll ballistic boundary handoff

- **Status:** Accepted
- **Date:** 2026-10-08
- **Related:** [ADR-0127](ADR-0127-exceptional-path-retention.md)
  (exceptional retirement),
  [ADR-0143](ADR-0143-flui-owned-input-event-vocabulary.md)
  (owned pointer and scroll vocabulary).

## Context

A bounded friction simulation completes at the content edge and then reports
zero velocity. Reading the completed animation's derivative loses the remaining
impulse. Nested scrollables also differ in axis, reversal, physics and mounted
owner lifetime. A release must use those actual policies without delivering
old work to a replacement occupying the same place in the tree.

## Decision

`ScrollPhysics::boundary_velocity` optionally reports the remaining velocity
at its hard boundary. The default returns no transfer, preserving local motion
for bouncing and custom policies unless they opt in. `ClampingScrollPhysics`
uses the existing `FrictionSimulation::time_at_x` and `dx`, with the same
friction coefficient and tolerance as its bounded ballistic run. This avoids
a second velocity estimator or a copied decay formula.

`Scrollable` publishes a private inherited endpoint containing its mounted
controller, animation driver, physics, axis, reversal and weak enclosing
endpoint. Controller, authored physics, axis or parent identity changes close
the outgoing endpoint; unmount does likewise. Default physics belongs to the
widget state, so an ordinary rebuild with the same configuration retains its
identity. Explicit physics uses the caller's `Arc` identity. There is no global
default instance, label comparison or registry of scroll owners.

The exact animation run's `TickerFuture` continuation owns transfer delivery.
Only normal completion at the expected boundary, with the same existing run
generation and live endpoint, can transfer. Cancellation or supersession cannot.
The continuation uses weak owner links and runs outside animation/scheduler
guards. Before delivery it takes and services any accepted pending controller
command. A same-pixel jump still owns a `Cancel` command even when no pixel
notification drains it: stopping cannot revise an animation outcome already
published before value listeners ran.

The child converts its residual impulse to physical direction; each enclosing
owner projects that impulse into its own scroll orientation. The nearest live
same-axis ancestor is tried using its real `apply_boundary_conditions` and
`create_ballistic_simulation` policies. A saturated clamping parent is skipped;
a bouncing parent at its outward extent can accept overscroll and is tried
before its grandparent. Orthogonal ancestors are traversed. A bouncing child
keeps its local recovery rather than handing off.

The presentation's measured device pixel ratio is captured at release and
carried through this same-window chain. The Send completion callback does not
capture the owner-local pipeline cell. Endpoints share immutable configuration
and a closed-owner authority bit, without adding per-node locks.
The parent's boundary-admission probe and simulation factory receive that same
measured ratio; a custom policy must not see a default ratio during admission
and a different ratio when its trajectory starts.

## Ownership and failure

Custom physics and activity callbacks execute without widget-state borrows.
Admission checks endpoint authority, current pixels and the existing animation
generation after each callback boundary. Transfer policy is computed before
the simulation factory returns its owned `Box<dyn Simulation>`: a failing
custom callback must not unwind through that arbitrary destructor. Healthy
simulations reach the animation controller's existing retirement boundary;
standalone retirement failures remain observable there. The first contained
failure stays authoritative and a subsequent gesture can make progress.

This does not make arbitrary aggregates safe against two destructors panicking
inside one destructor before the subsystem's containment boundary is reached.
Nor does it change `ChangeNotifier`'s pixel-listener policy: listener failures
are diagnosed in registration order, remaining listeners run, and the frame
pump is not promised to propagate those listener panics.

## Accessibility geometry

`Scrollable` also consumes the semantics owner's descendant ShowOnScreen
delivery. Its target and viewport rectangles are root-logical published
geometry. The receiving ancestor's published scroll-position basis accompanies
them. Before computing the nearest obscured edge, the widget rebases the target
by the signed difference between current and published pixels, including
reversal. This permits same-pipeline sibling requests before layout refresh
without applying an old displacement twice. Movement uses the existing clamped
controller/activity path and stops the previous trajectory. Membership and
ancestor callback delivery remain owned by the semantics layer.

## Observable contract

The public `scroll_physics_and_activity` table in
`crates/flui-widgets/tests/contracts.rs` mounts the witnesses in
`crates/flui-widgets/tests/scroll.rs`:

- `nested_fling_hands_remaining_velocity_to_matching_parent_axes` and
  `nested_fling_projects_reversed_child_and_preserves_orthogonal_and_bounce_policy`;
- `nested_fling_parent_boundary_policy_receives_presentation_pixel_ratio`,
  using the host's real scale-change ingress with ordinary and density-sensitive
  parent policies;
- `nested_fling_skips_saturated_parent_and_reentrant_jump_retires_transfer` and
  `nested_fling_bouncing_parent_at_extent_absorbs_before_grandparent`;
- `replacing_parent_invalidates_old_fling_handoff_and_next_gesture_recovers`,
  `nested_fling_same_controller_rebuild_preserves_accepted_handoff` and
  `nested_fling_equal_edge_jump_cancels_old_handoff_and_next_gesture_recovers`;
- `nested_fling_failure_keeps_first_panic_and_a_new_gesture_makes_progress` and
  `nested_fling_custom_physics_failure_and_retirement_preserve_first_and_recover`;
- `show_on_screen_reveals_offscreen_targets_on_both_axes_and_reverse`,
  `show_on_screen_same_pipeline_reentry_keeps_one_reveal_and_recovers` and
  `show_on_screen_sibling_reentry_delivers_last_target_without_stale_motion`.

No public nested-scroll coordinator is introduced. This decision neither
supplies system preferences nor infers hardware capabilities from display
names or wheel distance. Authored widget policy and measured input metadata
remain distinct from a future platform preference source.
