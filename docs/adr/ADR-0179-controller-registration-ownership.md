# ADR-0179: Controller registrations belong to driven owners

- **Status:** Accepted
- **Date:** 2026-10-08
- **Supersedes:** [ADR-0125](ADR-0125-vsync-registration-authority.md), public
  controller admission and explicit caller-managed controller removal only.
- **Related:** [ADR-0175](ADR-0175-owner-local-scheduling-and-animation.md),
  [ADR-0178](ADR-0178-notification-first-failure-custody.md).

## Decision

`AnimationControllerBuilder::build_on` returns a `DrivenController`. That owner
holds the controller's registry seat and withdraws it before cancellation on
disposal or drop. Controller clones observe and operate the same kernel without
owning the seat. `DrivenController::rebind` migrates that seat and preserves the
last sampled elapsed time; missing clocks settle finite runs and park repeats.

Public manual controller registration and removal are removed. The production
registry admits controllers only through the owning handle's internal path.
Crate-private identity and retirement tests exercise that same production
admission path. Keeping a second public admission path would leave callers responsible
for coupling registration and cancellation, defeating the owning API.

Child attachment remains an explicit public operation with a borrowed removal
token. Registry identity, non-reused slots, foreign and stale token refusal,
permanent exhaustion and child order retain ADR-0125's rules.

## Verification

The facade compiler suite rejects
`registering_a_bare_controller_does_not_compile` and accepts `owner_callbacks`
using `build_on`. Animation's `driven_controller_owns_its_seat_and_run` and the
mounted widget/runtime matrices exercise the public ownership path.
`registration_identity_and_retirement` retains the private foreign, stale,
expired and wrong-kind token cases, destruction reentry, competing retirement
failures and subsequent clock ticks. `vsync_nesting_and_reentrancy` preserves
the terminal capacity matrix and public owning-handle recovery after refusal.
