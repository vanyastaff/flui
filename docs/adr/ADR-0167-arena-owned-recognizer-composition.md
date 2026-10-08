# ADR-0167: Arena-owned recognizer composition

- **Status:** Accepted
- **Date:** 2026-10-07
- **Related:** [ADR-0127](ADR-0127-exceptional-path-retention.md),
  [ADR-0159](ADR-0159-gesture-arena-owner-delivery.md),
  [ADR-0161](ADR-0161-immutable-owner-local-gesture-recognizers.md)

## Context

A recognizer relationship must constrain the presentation arena's verdict.
Filtering callbacks cannot retract a verdict, and an independent resolver loses
the presentation's pointer generations and deadlines. Tap creates a member for
each contact, so recognizer allocation identity alone cannot describe which
contact participates in a relationship.

## Decision

`GestureArena::compose(GestureCompetition)` returns ordered `GestureBranches`.
Their arena handles share the presentation's clock, deadlines, owner closure and
pointer slots. Each admission inherits its handle's branch, including Tap's
per-contact proxy. An owner-local `Rc` allocation identifies the relation and
remains alive while any handle or admitted slot needs it. A relation owns no
recognizers, uses no process-global identity counter and cannot be confused with
a later allocation while its identity remains live.

`Exclusive` preserves the presentation's single-winner rule.
`RequireFirstFailure` retains the second branch's accepted request while any
first-branch member remains viable. Every first-branch member must reject or die
before fallback may win. Missing first-branch admission counts as failure after
close. Votes made during open admission are rechecked after all members join;
a repeated request is idempotent. A composed member cannot also join another
branch or the root in the same contact generation.

A sweep cannot grant a blocked fallback. It retains that exact generation,
allowing the next Down with the same PointerId to open a fresh slot. Deadline
polling, rejection or deferred dead-owner pruning releases eligible delivery
debt without another pointer event. Old entry handles cannot resolve a fresh
generation. Cancellation abandons the competition instead of selecting a
winner. Public explicit resolution also observes the failure dependency.

Pending verdicts hold weak identities. State and exact-slot removal commit before
notifications; recipients upgrade immediately before invocation outside borrows.
Existing explicit-resolution and sweep ordering remain in force. Callback and
retirement containment keeps the first failure authoritative under ADR-0127.

The initial contract is binary. Composing a branch returns matchable
`CompositionError::AlreadyComposed`; arbitrary nested dependency graphs are
unsupported. Independent detectors may each compose from the same uncomposed
presentation root.

`GestureDetector` expresses DoubleTap-preferred / Tap-fallback with these
handles. DoubleTap retains its first-Up hold because the hold preserves its own
timer and second-contact opportunity independently from fallback eligibility.
A detector inside a caller-provided composed `GestureArenaScope` inherits its
enclosing branch and preserves this hold behavior without constructing a nested
relation or panicking on that configuration.

`GestureDetector::exclusive_drags()` explicitly permits Pan and Horizontal drag
callbacks to compete for one winner. The first claimant wins; the default retains
the configuration conflict diagnostic. Changing the option replaces both actors,
commits their stable weak attachment targets, then cancels outgoing actors outside
borrows. Horizontal drag acquires no invented orthogonal-rejection behavior.

## Consequences and verification

Strong gesture teams, recognizer-allocation keys, independent child arenas and
callback filters are unnecessary. The relation uses the existing arena admission,
deadline, verdict and containment machinery.

The public `gesture_lifecycle_matrix` contains
`fallback_acceptance_waits_for_preferred_failure`,
`sweep_cannot_grant_a_dependency_blocked_fallback`,
`preferred_death_releases_accepted_fallback_debt`,
`preferred_deadline_releases_fallback_without_input`,
`composition_checks_preferred_members_after_close`,
`preferred_acceptance_rejects_a_pending_fallback`,
`composition_preserves_first_callback_failure_and_recovers`, and
`an_aliased_branch_join_cannot_change_the_contact_relationship`.
Removing inherited branch admission fails these behavioral witnesses.

The widget `pointer_and_gesture_recognition` table contains
`exclusive_drag_callbacks_have_one_arena_winner` and
`a_detector_in_a_composed_scope_preserves_double_tap_timing`, alongside
`double_tap_combined_with_tap_fires_double_tap_once_and_tap_never`.
The existing double-tap witness pins preserved behavior; it does not claim the
prior hold-based implementation was broken.
