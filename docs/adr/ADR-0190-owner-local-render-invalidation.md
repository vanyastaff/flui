# ADR-0190: UI render invalidation is retained by its attachment owner

- **Status:** Accepted
- **Date:** 2026-10-10
- **Supersedes:** ADR-0013's attach handle type, bounded-channel delivery for UI subscriptions, and single public invalidation capability. Its lifecycle and least-privilege rules remain.
- **Related:** [ADR-0175](ADR-0175-owner-local-scheduling-and-animation.md).

## Context

Animation completion can be the last notification a render subscription receives.
Sending its dirty marks through the external producer's bounded channel can reject
that notification under backpressure. Another animation tick cannot retry a final
sample once the controller becomes idle. Publishing the sample alone also leaves
a retained repaint boundary showing its previous effect layer.

A sample may require several phases. Calling the frame wake between two marks
allows a panic to leave paint admitted while semantics or compositing is missing.
The subscription must publish the sample and admit its complete update first.

## Decision

`RenderBox::attach`, `RenderSliver::attach`, and the erased render-object seam
receive `LocalRenderInvalidationHandle`. The capability belongs to one attachment
epoch and uses owner-local `Rc`/`Cell` storage. It cannot cross threads. UI
subscriptions, including animation, painter, flow, viewport, and floating-header
subscriptions, use this capability.

`request_update(RenderUpdateImpact)` admits every required phase together before
invoking the visual-update callback. Pending marks coalesce by attachment and
phase. The owner queues weak metadata once per pending attachment and replays it
through the existing canonical dirty-mark paths at frame entry. No callback,
animation value, or render object is stored in the pending queue.

Queue and notifier borrows end before the wake callback. Missing or panicking
wakes do not remove accepted marks. Installing a replacement wake retries pending
local work. Local draining contains phase failures and continues its accepted
tail while preserving the first failure. Detach withdraws the capability and its
queued metadata before user cleanup; reattach mints a fresh epoch. Owner release
and withdrawn attachments refuse new marks with `DirtySendError::OwnerGone`.
The inbox retires before user-owned pipeline resources. Wake delivery holds no
strong inbox reference, so releasing the pipeline from its wake revokes all peer
attachments before any user destructor can submit new work.

External producers retain `RenderInvalidationHandle`, its `Send + Sync` boundary,
and explicit bounded-channel backpressure. Both lanes converge on canonical
pipeline invalidation; neither mutates an in-flight render walk.

Box and sliver opacity share generation-checked owner-local sampling. A sample
commits before its wake, and an older reentrant read cannot overwrite a newer
sample. Animated transforms admit visual and semantics phases together.

## Alternatives

- Retry on another animation tick: completed runs have no further sample.
- Publish the sample before the bounded send: retained effects stay stale when
  admission is rejected.
- Shared overflow queue: adds locking and couples UI subscriptions to external
  producer pressure and retirement.
- Second optional attach hook: makes render objects choose the delivery lane.
  One owner-local attach contract encodes that choice in its type.

## Verification

`harness_animated_opacity_completion_survives_dirty_backpressure` and
`harness_animated_opacity_completion_survives_wake_failure` observe the final
visible frame inside a retained boundary after the controller becomes idle.
Their sliver counterparts exercise the contract through a viewport.
`harness_animated_transform_completion_keeps_semantics_after_wake_failure`
compares completed accessible geometry with a freshly mounted final sample.

The rendering `lifecycle_matrix` includes
`local_marks_survive_a_failed_wake_and_coalesce` and
`local_marks_wait_for_a_hook_and_end_with_the_attachment`: phases survive wake
replacement, coalesce, and stop at removal or owner release.
`local_drain_failure_preserves_the_phase_tail_and_next_frame` checks competing
phase wake failures, first-failure priority, and fresh-frame progress.
`owner_release_withdraws_peer_handles_before_user_drop` exercises saved peer
handles from user destructors during direct owner release and release inside
an active wake.

## Compatibility

Custom render objects change their attach parameter to
`LocalRenderInvalidationHandle`. Worker integrations continue acquiring the
external capability through `PipelineOwner::render_invalidation_handle`.
The layer dependencies and scheduler ownership are unchanged.
