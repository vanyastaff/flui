# ADR-0190: UI render invalidation is retained by its attachment owner

- **Status:** Accepted (2026-10-11)
- **Date:** 2026-10-10
- **Supersedes:** [ADR-0013](ADR-0013-render-object-attach-self-dirty-handle.md) in full: its attach handle type, bounded-channel delivery for UI subscriptions and single public invalidation capability are replaced by the Decision, and the lifecycle rules that remain true are restated under "Attachment lifecycle".
- **Related:** [ADR-0175](ADR-0175-owner-local-scheduling-and-animation.md).
- **Revised:** 2026-11-03 — the attachment lifecycle that [ADR-0013](ADR-0013-render-object-attach-self-dirty-handle.md) set is restated here, so this record holds the whole rule. ADR-0013's rule that `flui-rendering` takes no dependency on `flui-scheduler` is not carried over: the crate depends on it for `PostFrameHandle` (`crates/flui-rendering/Cargo.toml:59`); the attach seam still carries no scheduler.

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

Subtree removal and relocation first withdraw every selected attachment, then
change membership and run parent-data and render-object cleanup. Withdrawal
cannot call user code. A private detaching state refuses reattachment until
that node's detach hook is ready to run; the hook receives an already-closed
interval. This makes peer handles refuse during the first cleanup, including
membership wakes, while nodes outside the selected batch remain attached.

External producers retain `RenderInvalidationHandle`, its `Send + Sync` boundary,
and explicit bounded-channel backpressure. Both lanes converge on canonical
pipeline invalidation; neither mutates an in-flight render walk.

Box and sliver opacity share generation-checked owner-local sampling. A sample
commits before its wake, and an older reentrant read cannot overwrite a newer
sample. Animated transforms admit visual and semantics phases together.

## Attachment lifecycle

The attach seam has one lifecycle, whichever lane delivers a mark.

- **A defaulted pair.** `RenderBox` and `RenderSliver` have
  `fn attach(&mut self, handle: LocalRenderInvalidationHandle)` and `fn detach(&mut self)`
  that do nothing unless overridden
  (`crates/flui-rendering/src/traits/render_box.rs:652`, `:661`;
  `crates/flui-rendering/src/traits/render_sliver.rs:484`, `:493`), forwarded by the blanket
  impls (`render_box.rs:981`, `render_sliver.rs:664`) to the erased seam
  (`crates/flui-rendering/src/traits/render_object.rs:876`, `:893`). Neither runs per
  frame, so a render object that does not override them pays nothing.
- **Attach follows insertion.** The owner calls `attach` once the node's id is minted and its
  links are wired, with a handle bound to the node's next `AttachmentEpoch`
  (`attach_inserted_node`, `crates/flui-rendering/src/pipeline/owner/accessors.rs:1751`;
  `RenderNode::attach`, `crates/flui-rendering/src/storage/node.rs:169`).
- **Detach covers removal and relocation.** `detach` runs for every node of a removed subtree
  (`PipelineOwner::remove_render_object`,
  `crates/flui-rendering/src/pipeline/owner/accessors.rs:714`) and of a relocated one.
  Relocation keeps the node ids and closes each attachment interval
  (`detach_render_subtrees`, `crates/flui-rendering/src/pipeline/owner/relocation.rs:302`).
- **The relocation token is consumed once.** `DetachedRenderSubtrees` is `#[must_use]` and
  has no `Clone` (`relocation.rs:25`-`:50`). `attach_render_subtrees` (`relocation.rs:421`) reopens every
  interval with a fresh epoch, and `release_detached_render_subtrees_for_finalization`
  (`relocation.rs:541`) hands the batch to ordinary unmount. A call that fails validation mutates
  nothing and returns the token in its failure value (`into_token`), so the batch is still
  released or retried.
- **A hook that panics.** The owner does not contain a panic from an `attach` or `detach`
  override; it propagates to the insertion, removal or relocation call. An interval opens before
  `attach` runs and closes before `detach` runs (`crates/flui-rendering/src/storage/node.rs:169`,
  `:206`), and removal and relocation withdraw admission for the whole batch before the first
  `detach` (`accessors.rs:732`, `relocation.rs:316`), so no handle of the batch admits work while
  a hook unwinds. A panic part-way through a relocation's mutation is fatal for that batch and is
  not rolled back (`relocation.rs:300`, `:420`).
- **The render object never sees a ticker.** `attach` carries the invalidation handle and
  nothing else: no scheduler, ticker provider or vsync. The owning view creates the
  `AnimationController`, where every other animated widget reaches its vsync, and passes it to the
  render object's constructor; the render object treats it as a `Listenable`. `flui-rendering`
  depends on `flui-animation` only in `[dev-dependencies]`
  (`crates/flui-rendering/Cargo.toml:130`).
- **Owned and external notifiers are one shape.** An owned animation
  (`RenderAnimatedSize`, `crates/flui-objects/src/layout/animated_size.rs:775`) and an external
  notifier (`RenderCustomPaint`, `crates/flui-objects/src/proxy/custom_paint.rs:423`) subscribe in
  `attach`, mark themselves on notify and unsubscribe in `detach`. They differ in who owns the
  `Listenable` and in which phase they mark. `detach` stops the subscription and nothing more;
  disposing the controller is the owning state's job.
- **The external lane replays through the same check.** A request sent through
  `RenderInvalidationHandle` carries its attachment epoch, and the frame-entry drain applies it
  only when the node is live and `is_attached_for(epoch)`; a request from before a detach, while
  detached or from a previous attachment is dropped
  (`crates/flui-rendering/src/pipeline/owner/accessors.rs:243`). A full channel returns
  `SendError::ChannelFull` (`crates/flui-rendering/src/pipeline/handle.rs:97`).

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

`subtree_cleanup_withdraws_all_selected_handles_before_user_code` exercises
parent-data detach and destruction, render hooks and membership wakes during
removal and relocation. It also checks surviving attachments and fresh handles
after reattachment. Restoring per-node withdrawal after parent-data cleanup
fails both rows because selected peer handles still admit work.

`insert_fires_exactly_one_attach_with_a_handle_bound_to_the_new_id` pins attach after
insertion, and `a_relocated_subtree_takes_its_new_depth` pins relocation through a detached
token (both in `crates/flui-rendering/tests/attach_detach_lifecycle.rs`, rows of
`lifecycle_matrix`). The token's no-`Clone` shape is a compile-time fact (the `compile_fail`
doctest on `DetachedRenderSubtrees`, `relocation.rs:35`), and the `flui-animation` edge is
visible in the manifest. No test pins that a failed attach returns the usable token; that row is
to be added to the same file.

## Compatibility

Custom render objects change their attach parameter to
`LocalRenderInvalidationHandle`. Worker integrations continue acquiring the
external capability through `PipelineOwner::render_invalidation_handle`.
The layer dependencies and scheduler ownership are unchanged.
