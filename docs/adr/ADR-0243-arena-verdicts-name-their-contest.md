# ADR-0243: Arena verdicts name their contest

- **Status:** Accepted
- **Date:** 2026-10-17
- **Related:** [ADR-0161](ADR-0161-immutable-owner-local-gesture-recognizers.md),
  [ADR-0208](ADR-0208-gesture-ownership-table-and-composition.md)

## Context

The arena keeps every contest in its own generation, and a contest a hold keeps
past its Up (a double tap's first click) stays in the arena while the same
pointer ID makes its next contact: a mouse is always one pointer. Its verdicts
still reached members as `accept_gesture(PointerId)` and
`reject_gesture(PointerId)`. A recognizer that keeps one arena member across
contacts (long press, swipe, force press) matched the verdict by pointer, so
settling the earlier contest cancelled, or accepted, the contact that was down.
Sweeping that contest also took the carried `Sequenced` successors of the newer
contact by pointer, so its hold could no longer hand the contact off.

## Decision

`GestureArenaMember::accept_gesture` and `reject_gesture` take the
`GestureContest` they settle: pointer, input sequence and arena generation.
`GestureArenaEntry::contest` names the contest an entry joined, and
`PrimaryContact::tracks_contest` compares a verdict with the contact's own
entry. A member that outlives one contact compares contests, never pointers.
Every verdict the arena delivers carries the contest that produced it,
including the verdicts of carried successors, and sweeping a contest ends only
the successors it carried.

A refused admission's entry names generation 0, which the arena never issues,
so no verdict settles it.

## Consequences

Every external `GestureArenaMember` implementation changes its two signatures;
a member that used the pointer reads `contest.pointer()`. The contest cannot be
constructed outside the crate, so a verdict cannot be forged by pointer.
`gesture_lifecycle_matrix` pins the cases:
`a_held_earlier_contest_cannot_end_the_next_contact` for long press, swipe and
force press, and `a_swept_earlier_generation_keeps_the_next_contacts_successors`
for the carried successors.
