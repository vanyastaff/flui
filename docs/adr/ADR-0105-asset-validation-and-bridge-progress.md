# ADR-0105: Image load admission and progress

- **Status:** Accepted (2026-10-03)
- **Date:** 2026-10-03
- **Superseded-by:** [ADR-0279](ADR-0279-image-loads-run-on-the-lanes-their-caller-holds.md) for runtime
  selection and execution; [ADR-0317](ADR-0317-the-asset-registry-is-image-only-and-the-facade-opts-into-it.md)
  for generic descriptor validation
- **Related:** [ADR-0047](ADR-0047-unified-execution-services.md),
  [ADR-0250](ADR-0250-asset-registry-keeps-decoded-images.md)
- **Revised:** 2026-11-07 — states the image-only admission and failure contract after the generic asset
  API was removed

## Context and problem statement

The original registry accepted arbitrary asset descriptors but did not consistently validate them, and
its image bridge could remain pending when it relied on an ambient current-thread runtime. The generic
validation decision and registry-owned fallback runtime have since been replaced. The remaining current
question is how a concrete image source is admitted, how progress is driven, and which failures a caller
observes.

## Decision

An image request is an `ImageSource`, not a user-defined descriptor with a validation callback. Cold
admission creates registry-owned shared work; a request for the same source joins it, and a completed
source resolves from the decoded-image LRU. Source identity and ticket generations, rather than generic
keys or validation hooks, determine which work and pixels a request observes.

`load_image` and `load_network_image` return `ImageLoad`. Execution uses the `LoadServices` of the
request under ADR-0279: no ambient current-thread runtime is assumed and the registry owns no fallback
runtime. With no lanes, polling the returned future drives progress.

A source that cannot be read or decoded resolves to `LoadFailed` or `DecodeFailed`. HTTP capability,
request and status failures use their dedicated variants, and lanes shutting down use `LanesRefused`.
Errors are not retained, so the next request can succeed. Dropping one subscriber does not cancel the
others; an unfinished run can be taken over by a remaining waiter.

## Behavioral evidence

- `image_loads_run_on_their_lanes_and_survive_the_requests_that_leave` checks lane and no-lane progress,
  missing-file recovery, sharing, cancellation and handover.
- `network_loads_fetch_with_the_callers_client_and_recover_after_errors` checks malformed-image recovery,
  no-client and HTTP failures.
- `assets_feature_exposes_and_loads_images_through_the_facade` drives a real external-consumer image to
  completion through the public facade.

## Consequences

There is no descriptor-validation hook to run on a cache hit and no registry runtime initialization to
fail. Source and execution errors remain ordinary operation results, and no-lane callers must poll their
futures. Generic custom-asset validation is outside the crate after ADR-0317.
