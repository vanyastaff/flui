# ADR-0197: Explicit animation completion preserves the admitted run

- **Status:** Accepted
- **Date:** 2026-10-11
- **Related:** [ADR-0064](ADR-0064-animation-completion-is-one-controller-resolved-future.md),
  [ADR-0179](ADR-0179-controller-registration-ownership.md),
  [ADR-0195](ADR-0195-presentation-motion-policy.md)

## Decision

`AnimationController::finish()` is an application-authoring command for completing
an admitted finite run immediately. It publishes the exact endpoint and the
directional settled phase, resolves the original future successfully, then delivers
value, status and completion observers through the existing delivery contract.
It admits no new run and consumes no run identity.

Finite repeats use the endpoint of their final leg. Native flings use their
declared bound; property motion uses its target. Generated value motion publishes
its exact target representation without invoking a converter or sampling a curve.
An infinite repeat or arbitrary custom simulation has no declared finite endpoint:
the command returns `AnimationError::NoFiniteEndpoint` without changing the run,
its published value or its future. An idle call is silent; a disposed call fails.

A paused finite run can finish. This explicit application command neither advances
the presentation clock nor changes playback rate. Policy settlement shares the
finite endpoint commit, while parking infinite work retains its original future.

The endpoint and original successful outcome commit before user code runs. Completed
simulations and curves leave the kernel under its borrow and retire after delivery
through the existing recovery custody. Listeners may replace the run or release its
owner. The replacement remains authoritative and owner release does not cancel an
already committed successful outcome. Callback and retirement failures preserve
the first failure and drain accepted delivery according to ADR-0064.

## Alternatives

`stop()` cancels the admitted run and leaves its current sample. Assigning the target
with `set_value()` also cancels. Starting a zero-duration replacement would cancel
the original future and consume a new identity. Sampling at an artificial extreme
timestamp cannot establish an arbitrary simulation's endpoint and can invoke user
code. None supplies successful completion of the original run.

Flutter's [`stop(canceled: false)`](https://github.com/flutter/flutter/blob/331a8527941976d8acf17f255695b5cf81be4faf/packages/flutter/lib/src/animation/animation_controller.dart#L870-L895)
successfully completes its future without moving the value or notifying value/status
listeners. FLUI deliberately requires endpoint publication.
[Web Animations finishing](https://www.w3.org/TR/web-animations-1/#finishing-an-animation)
also preserves successful completion, but refuses zero playback rate. FLUI's paused
completion is deliberate native authoring behavior; this is not a claim of complete
Web Animations conformance.

## Evidence

The public `controller_robustness_contract` rows cover finite forward/reverse motion,
paused motion, finite repeat parity, native physical endpoints, atomic refusal,
reentrant replacement, owner release, failed wakers and source retirement custody.
`status_delivery_failure_custody` exercises completion capture and waiter retention
and competing failures through both natural and explicit completion.
`explicit_finish_commits_an_authored_page_and_retires_the_old_route` mounts an
application-authored `Route` through public navigator traits, checks submitted
paint and the original successful future, and verifies route disposal and subsequent
navigation. This application-authoring surface is explicitly approved even though
the built-in routes do not expose their private controller handle.
