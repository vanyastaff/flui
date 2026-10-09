# ADR-0182: Gesture velocity and contact ownership across settle

Status: Accepted

## Context

Gesture callbacks report velocity in logical pixels per second. Controllers
operate in their authored value range. Each consumer converting between them
can lose the physical velocity, overflow an intermediate calculation or reuse
the release direction as the position direction. A drawer that swaps its edge
detector for a panel detector also retires the captured contact when a frame
rebuilds the newly visible panel.

## Decision

`AnimationController::fling_across(velocity, extent)` owns the conversion:
`velocity * (upper - lower) / extent`. The extent is finite and strictly
positive. Non-finite or unrepresentable rates refuse before run mutation;
refusal preserves the previous run and its completion obligation. Product and
division associations are checked to avoid intermediate overflow and underflow.
Ordinary controller `fling` retains its value-units-per-second contract.

Dismissible and Drawer use this admission for gesture release. The dragged
position's sign remains independent of the release velocity: moving back
toward the origin cannot move a card instantly to the opposite side.

Drawer keeps one gesture owner while its child switches between the closed
edge strip and open panel. The closed hit extent stays restricted to the
configured edge; opening expands it to the panel and scrim without replacing
the captured route. Cancellation still settles by position without momentum.

This contract does not choose the consumer's extent. Drawer uses its declared
panel width capped by available width. Dismissible's existing constraints-based
extent is exact under tight constraints; actual child sizing under loose
constraints remains a separate geometry requirement.

## Evidence

`retarget_seams` mounts
`fling_across_hands_the_gesture_velocity_to_the_spring`: signed velocities,
non-unit ranges, representable extreme products, refused extents and continued
delivery of the previous run. Screen velocity is checked from position samples.

The mounted `animation_and_visibility` table measures painted card coordinates
in `a_dismissible_release_keeps_finger_speed_on_any_width`, including backward
release and the position seam on two widths.

Material's `overlay_contracts` table mounts
`a_drawer_release_keeps_finger_speed`. It pumps between pointer moves and checks
painted position and release velocity for both edges, authored widths and a
panel constrained below its authored width. Existing cancellation, closed-edge
hit testing and admitted gesture-profile cases remain part of the same table.
