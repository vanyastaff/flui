# ADR-0100: Prepared GPU work and retained frame commit

- **Status:** Accepted
- **Date:** 2026-10-01
- **Supersedes:** ADR-0087 §4 only where it permits rendering into and invalidating
  the previous retained image before a replacement frame is prepared and submitted.
- **Related:** ADR-0006 (record/replay), ADR-0068 (presentation disposition),
  ADR-0091 (realm and GPU ownership).

## Context

The engine submits clear, backdrop, mask and blit work before its final encoder.
A late failure cannot undo these submissions. Invalidating an already modified
retained image forces a full repaint but cannot recover the last committed pixels.
CPU frame maintenance is also earlier than GPU completion: releasing resource
accounting there permits outstanding GPU work to exceed a stated quota.

## Decision

One instance-owned DeviceDomain coordinates managed painters on one device.
Preparation reserves resources before allocation and carries charges with the
prepared work. A consuming submission value binds commands and charges to that
owner; every production submission uses it. Submitted charges retire through GPU
completion, unsubmitted charges through discard. Quotas initially cover explicitly
listed prepared resources; legacy allocations are not represented as a total VRAM cap.

A retained frame renders into a distinct candidate. A partial candidate is seeded
from the prior committed image before damage is repainted. Both images and the copy
are admitted resources. A CPU preparation/submission failure discards the candidate
without changing the committed image; resources used by earlier submissions still
retire. Known damage remains owed. A successful submission sequence promotes the
candidate; this is a queue-order commitment, not physical display acknowledgment.
Device faults invalidate the generation: no previous image is promised usable after
device loss. GPU completion and surface present remain distinct observations.

This first implementation preserves the synchronous frame path and the existing
PresentDisposition meaning. It does not await GPU completion inside layout/paint.
Future presentation receipt changes require a separate cross-crate decision.

Raw Device/Queue access remains a trusted embedding interface. Scoped Rust
references do not prevent clone, destroy, or arbitrary submissions. Managed
accounting is enforced through the owner, not claimed as isolation from raw callers.

The initial GPU quota covers immutable viewport/gradient bindings, tessellated
clip preparation, offscreen blur/mask parameters and retained candidate images.
Existing buffer/texture pools, atlases, caches and external allocations are outside
it. Recording has a separate per-painter arena-capacity/live-element budget,
shared across seals and scratch copies; it excludes allocator slack, DrawItem
metadata and upstream tessellation/image-processing allocations.

Managed outer frames own a submission scope. Work emitted by that synchronous
frame cannot rely on future browser callbacks to make an impossible footprint
fit. Such refusal is a hard resource error; still-pending earlier frames may cause
transient backpressure. The initial scope also bounds total frame submissions,
independently of callbacks that happen to complete during native polling.

The prior-work backlog window is checked before a frame starts, not against every
submission of that frame. Its default is 64 pending submissions. The independent
cumulative frame allowance derives from the prepared object and CPU metadata
profile (65,536 objects and 128 MiB by default). Each submission additionally
reserves one metadata object and checked CPU bookkeeping bytes until completion.
This permits ordinary effect-heavy scenes while still bounding an endless public
frame even when callbacks retire continuously. These are requested bookkeeping
charges, not allocator or driver memory measurements. Transient refusal preserves
runtime frame demand under ADR-0101.
Allocation-retirement callbacks also count as pending release independently of
submission callbacks: on WebGPU, dropping an incompatible spare after resize may
register a callback that cannot run until the current synchronous turn ends. A
footprint that fits after this release is transient pressure; a known impossible
committed-plus-candidate footprint is still rejected before admission.

Submission and callback-registration panics quarantine accounting on the owner
and invalidate its domain. An early callback is latched until registration
succeeds. The first panic stays authoritative if both operations fail. Normal
completion releases a charge once. Quarantined bookkeeping survives until owner
teardown; its destruction does not report GPU completion or reclaimed VRAM.
The owner provides a final nonblocking progress opportunity. A persistent closing
service and bounded diagnostic resolver remain separate work; this decision does
not promise continued polling of raw device clones after their owner disappears.

## Verification

Public `WgpuPainter::begin_frame` is fallible and owns the same cumulative
submission scope until `finish_frame`; callback retirement cannot renew that
allowance mid-frame. Internal child painters only reset recording under their
parent's scope. Domain quarantine participates directly in the backend loss
predicate used by the native recovery loop, independently of the driver callback.

- Fail after an early submitted write to a candidate; previous committed readback
  stays unchanged, then a valid frame progresses with the owed damage.
- A candidate and its submitted uses keep one charge through resize, replacement,
  discard and delayed completion; no CPU finish or timeout claims GPU completion.
- Two different blur calls and viewport sizes before final submission retain their
  own parameters. First failure propagates through the layer walk.
- Parent and offscreen painters share admission/ordering; independent device owners
  reject mixed charges. Closing/loss and late callbacks cannot double-release.
- Compare retained-frame memory/copy cost and ordered rendering against the recorded
  baseline. Update architecture and supersession metadata when the implementation
  passes its behavior and recovery tests.

The executable checks are `gradients_read_back_as_specified`,
`painter_images_and_offscreen_results_read_back_as_specified`,
`prepared_quota_failure_and_retirement`,
`an_invalid_target_promotes_to_full` and `every_failure_boundary_retires_its_ticket`.
Private quota/fault seams exercise inaccessible failures; rendering assertions use
actual GPU readbacks. The [measurement record](../research/engine-foundation-measurements.ru.md)
distinguishes measured round trips, the candidate cost model and unmeasured targets.
