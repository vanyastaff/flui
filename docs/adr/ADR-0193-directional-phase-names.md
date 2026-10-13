# ADR-0193: Animation names distinguish phase, position and run outcome

- **Status:** Accepted
- **Date:** 2026-10-10
- **Supersedes:** [ADR-0177](ADR-0177-controller-ordered-delivery.md), settled-phase vocabulary only.
- **Superseded-by:** [ADR-0219](ADR-0219-required-observer-registration.md), the `Animation`
  phase helpers only.
- **Related:** [ADR-0179](ADR-0179-controller-registration-ownership.md), [ADR-0180](ADR-0180-source-bound-animation-status-subscriptions.md).

## Context

An animation can stop at an interior value. Its directional phase settles while
the interrupted run resolves as cancellation. The names `Completed` and
`Dismissed` invite consumers to infer successful arrival at the upper or lower
bound. Those are separate questions: a drawer interrupted while closing must
retain its visible panel until its value reaches zero and its run ends.

## Decision

`AnimationStatus` retains four phases: `Forward`, `Reverse`, `ForwardSettled` and
`ReverseSettled`. The latter two replace `Completed` and `Dismissed`. The matching
predicates are `is_forward_settled()` and `is_reverse_settled()`, including the
default helpers on `Animation`. The default phase remains reverse-settled.

The rename preserves transitions, delivery ordering, reversal and cancellation.
Consumers inspect the value for position, `is_animating()` for ownership of a run
(including a paused run), and `AnimationRunFuture` for its outcome. A settled
phase implies neither endpoint arrival nor successful completion. Route results,
gesture completion and other outcomes retain their own completion vocabulary.

`ConstantAnimation::forward_settled(value)` and `reverse_settled(value)` accept
any constant value; they do not select an endpoint. `UNIT_START_ANIMATION` and
`UNIT_END_ANIMATION` name the fixed normalized values zero and one. They are
constants rather than process-global animation identities. Navigation uses them
for its absent-peer and offstage fallback values.

This is an intentional pre-1.0 Rust API and optional serde wire break. Serde emits
`"ForwardSettled"` and `"ReverseSettled"` in place of the former spellings;
`"Forward"` and `"Reverse"` remain unchanged. Persisted phase values must be
migrated. No compatibility aliases retain the misleading names.

## Alternatives

Keeping the names and expanding comments leaves endpoint assumptions easy to
make at each consumer. Adding endpoint flags duplicates position and introduces
states that must be synchronized. Expanding this enum with run outcomes mixes
independently observable contracts and cannot represent every generic value's
endpoint. Naming the existing phases precisely avoids those responsibilities.

## Verification and limits

The public `a_constant_phase_does_not_install_a_run` case exercises every phase
at an interior value through animation combinators. `status_delivery_contract`
pins reentrant transition order; `directional_phase_wire_names` pins the serde
spellings and rejects the former names. The controller source and run ownership cases
pin cancellation and installed runs. `overlay_contracts` includes
`grabbing_a_closing_drawer_preserves_its_panel`, exercising interruption on both
drawer sides and both sides of the settling threshold through public pointer
events and committed paint.

The names describe FLUI's existing contract. They are not a claim that this
enum implements the Web Animations `playState` interface.
