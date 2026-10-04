# ADR-0112: Gesture completion carries an explicit terminal reason

- **Status:** Accepted
- **Date:** 2026-10-04
- **Related:** ADR-0086 (event-context callback delivery), ADR-0088 (the package-author surface)

## Context

An accepted drag's pointer cancellation currently invokes its end callback with
the measured velocity, as an ordinary pointer release does. The cancel callback
is reserved for a gesture rejected before acceptance. That callback distinction
is already part of FLUI's public gesture behavior.

End consumers cannot infer intent from the sampled velocity or final position.
A cancelled fast scroll starts a ballistic run; cancelling a refresh pull past
its threshold starts an operation; cancelling a dismiss or a back gesture past
its positional threshold can remove content or navigate. Adding a cancel
callback to those widgets does not reach the accepted-cancellation path.

## Alternatives

- Change accepted cancellation to invoke the existing cancel callback. Rejected:
  this changes the established accepted-end protocol and makes callers' end
  cleanup disappear or duplicate if both callbacks run.
- Replace the velocity of every cancelled drag with zero. Rejected: positional
  thresholds can still commit dismissals and navigation, and observers lose the
  measured motion without learning why the gesture ended.
- Add independent booleans and callback conventions to each widget. Rejected:
  the same terminal distinction belongs in the interaction contract and must
  reach package consumers too.

## Decision

`GestureEndReason` is a closed enum with `Completed` and `Cancelled` variants.
`DragEndDetails` carries `reason` alongside its measured velocities. Pointer up
produces `Completed`; pointer cancellation after acceptance produces
`Cancelled`. Both invoke the existing end callback. Pre-acceptance rejection
keeps the existing cancel callback and does not terminate an unrelated running
animation. Recognizer terminal state is cleared before user callback delivery.

`InteractionEndDetails` also carries this reason. A viewer's pan end forwards
the actual drag reason. Its synthetic discrete wheel and pan-zoom interactions
use `Completed`; that variant describes normal completion, not necessarily a
physical pointer release. The enum is available through the interaction,
widgets, facade and SDK surfaces, and the SDK's pinned surface records it.

Consumers use the reason explicitly:

- A cancelled scroll has no release impulse. An in-range position becomes idle;
  an overscrolled bouncing position may spring back with zero impulse.
- A cancelled refresh pull clears its pull distance and starts no refresh or
  release impulse; an out-of-range position may recover with zero velocity.
- A cancelled dismiss returns the content to its resting position and does not
  call dismissal completion, including cancellation at a positional threshold.
- A cancelled back gesture restores the current route regardless of position;
  cancellation cannot commit a pop.
- A drawer settles according to its position without the cancelled gesture's
  velocity impulse, consistent with its existing cancellation policy.
- Viewer observers receive the reason and can choose their own terminal policy.

This changes public struct construction and terminal behavior. It does not
change pointer routing, arena arbitration, multi-drag's separate cancel
protocol, or the event-context write boundary. Foreign callbacks remain outside
recognizer/state locks; the existing callback panic policy remains in force.

## Validation

Consumer tests must dispatch actual pointer cancellation after recognition and
advance virtual frames. They must distinguish cancellation from a normal fast
release, assert the resulting activity/content/route, and show that a following
normal gesture still works. The interaction suite retains its accepted-end
callback contract and additionally checks the terminal reason. Source controls
that treat `Cancelled` as normal completion must fail those consumer cases.

The public widget families include
`horizontal_drag_pointer_cancel_after_acceptance_ends_and_does_not_wedge_the_detector`,
`viewer_reports_cancelled_then_completed_interactions`,
`cancelling_an_in_range_scroll_ends_activity_without_coasting`,
`cancelling_bouncing_overscroll_settles_without_release_velocity`,
`cancelling_a_threshold_refresh_pull_does_not_refresh`,
`a_cancelled_horizontal_dismiss_restores_the_card`,
`a_cancelled_vertical_dismiss_restores_the_card`,
`cancelling_a_fully_slid_card_restores_it_without_dismissal` and
`cancelling_a_back_swipe_past_halfway_keeps_the_route`. Material's overlay family
adds `cancelled_fast_edge_drag_settles_closed_below_halfway` and
`cancelled_fast_panel_drag_settles_open_above_halfway`. These consumers exercise
actual pointer delivery and virtual frames; the fully slid card case first
asserts that the card has moved outside the original hit region.

The design starts from FLUI's existing callback and synchronous frame contracts;
another framework's choice of cancel callback is not a reason to silently
replace them. The consumer inventory covers every production end handler before
changing the cross-crate contract.
