# ADR-0006: C-IR record/replay seam — explicit Command-IR as data, wgpu stays concrete

- **Status:** Accepted
- **Date:** 2026-06-17

## Context

`WgpuPainter` had grown to ~6,400 lines and ~58 fields mixing batch recording, save-layer state
machines, gradient construction, text integration and per-frame submission, with no internal
seam. Among the renderers surveyed (Impeller, Vello, WebRender, Bevy, egui/epaint, GPUI), FLUI
was the only one with no record/replay boundary. The behavior was correct and audited; the
structure blocked maintenance, testing, batching and retention.

## Decision

**1. The Command-IR is explicit data.** `DrawSegment` / `DrawItem` (`command_ir.rs`) are plain
structs. The backend walks the `flui_layer::Scene`; record methods write the IR; replay reads it
and emits GPU commands. This is the shape Impeller took when it replaced its retained entity
tree with "display list in, flat command vector out".

**2. Record and replay are separate components.**

- `DrawBatcher` (`batches/`, one file per primitive family: shapes, gradients, paths, images,
  text) owns recording.
- `GpuReplay` (`replay/`) owns the emit/submit path: GPU plumbing (viewport buffer and bind
  group, unit quads, default sampler), the segment-flush families, the submit dispatch loop,
  opacity-layer flushing and offscreen reintegration.
- `GpuStateStack` owns the transform/scissor/clip stacks; `LayerCompositor` owns save-layer and
  opacity-layer state.
- `WgpuPainter` coordinates: finish recording, then `replay.submit(...)`.

**3. The borrow checker enforces the seam.** `GpuStateStack` exposes only `Copy` accessors, so
no caller can hold a borrow into it across an IR write. Record methods take the narrowest
disjoint set of borrowed parameters they need (`&mut DrawSegment`, `&GpuStateStack`,
`&mut GpuResources`, opacity), never `&mut WgpuPainter`. No locks.

**4. One `Matrix4` ↔ glam edge.** `GpuStateStack` stores `glam::Mat4`, which matches the GPU
buffer layout. `flui_types::Matrix4` is converted once, where the layer dispatcher hands a
transform to the command renderer; `batches/`, `pipeline_set.rs` and `replay/` never name
`Matrix4`.

**5. wgpu stays concrete.** There is no device or backend trait. A second GPU backend, if one
ever has a consumer, is a second concrete implementation behind the same `Scene` input, not a
trait designed today for a backend that does not exist.

**6. Replay is deterministic.** Recording holds no `&mut` device, queue or encoder; replaying
the same IR to two encoders emits the same command stream (`deterministic_replay_tests.rs`).
Text is recorded like every other primitive: paragraphs arrive shaped (ADR-0065) and are placed
through the engine-owned glyph atlas (ADR-0067).

## Consequences

- The painter is a thin coordinator; recording and replay can each be read, tested and changed
  without opening the other.
- The IR is the natural place for batching, retention and, later, parallel encoding.
- Adding a primitive means a record method in its family file plus a flush family in replay —
  two known places.

## Alternatives rejected

| Option | Why rejected |
|---|---|
| A `dyn GpuBackend` device trait | No second backend exists; it would re-type thousands of lines of audited behavior for a hypothetical consumer. |
| Keep the monolithic painter | No record/replay, no batching boundary, no testable seam. |
| A separate IR crate | No second consumer; extractable later if one appears. |
| Push-constants for per-draw uniforms | Too small by default for transform + paint and not available on WebGPU; instancing and dynamic-offset/storage buffers are what the field uses. |
