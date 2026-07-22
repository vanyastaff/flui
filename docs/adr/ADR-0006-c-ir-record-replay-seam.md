# ADR-0006: C-IR record/replay seam — explicit Command-IR as data, wgpu stays concrete, record side lives in DrawBatcher

*Establish an explicit record/replay Command-IR as data so the engine gains batching, retention, and parallel-encode capacity without a device/backend trait abstraction — keeping wgpu concrete and decomposing WgpuPainter into cohesive components with enforceable boundary rules.*

---

- **Status:** Accepted (record/replay split fully shipped T7–T10; C1 closed — painter.rs = 1 432 non-test LOC; T11 deterministic-replay test remains)
- **Date:** 2026-06-17
- **Deciders:** @vanyastaff
- **Scope:** `crates/flui-engine` — `src/wgpu/batches/`, `src/wgpu/state_stack.rs`, `src/wgpu/layer_compositor.rs`, `src/wgpu/pipelines.rs`, `src/wgpu/painter.rs`, `src/wgpu/replay.rs`, `src/wgpu/backend.rs`
- **Spec reference:** `.rust-studio/specs/flui-engine-overhaul/spec.md` (C-IR approach, tasks 7–11)
- **Supersedes:** `adr-flui-engine-decomposition.md` if it exists (that draft argued "no IR"; this ADR supersedes that position — the IR is explicit data, not a future option)

---

## Verdict

**Target architecture (one paragraph).** The engine uses an explicit **record/replay Command-IR as data**: `Backend` visits the `flui_layer::Scene` and calls `WgpuPainter` record methods, which write into `DrawSegment` / `DrawItem` structs (the Command IR) in `command_ir.rs`. `DrawBatcher` owns the record methods — one file per primitive family (`batches/{shapes,gradients,paths,images}.rs`) — and accepts **disjoint borrowed parameters** (`&mut DrawSegment`, `&mut Vec<DrawItem>`, `&GpuStateStack`, `&mut GpuResources`, `opacity: f32`) so the borrow-checker enforces the data-flow contract without locks. `GpuReplay` (`replay.rs`) owns the replay/submit path: it holds the 5 GPU-plumbing fields (viewport buffer, viewport bind group, unit quads, default sampler), the texture-batch scratch, the 6 segment-flush families, and the top-level `submit` dispatch loop plus `flush_opacity_layer` recursion and `reintegrate_offscreen_content`. `WgpuPainter` is a thin coordinator: record-finish + `self.replay.submit(...)`. wgpu stays concrete — no device trait, no second backend. The engine is the only abstraction flui needs between `Scene` and GPU calls.

---

## Context

### The problem: WgpuPainter was a god-object

`WgpuPainter` at the start of this overhaul was ~6 410 lines and ~58 fields, mixing batch recording, save-layer state machines, gradient construction, text-rendering integration, and per-frame submission with no internal seams. Market research established that flui was the only serious renderer (vs Impeller, Vello, WebRender, Bevy, egui/epaint, GPUI) with no record/replay IR seam. The five wgpu-29 audit rounds had proven the behavior correct; the structure blocked maintainability, testability, and future capabilities.

### What shipped (T7–T10)

| Task | What landed | PR |
|---|---|---|
| T7 | `GpuStateStack` — owns the 4 paired transform/scissor/rrect-clip/rsuperellipse stacks; Copy accessors; glam-internal; single `Matrix4`↔glam conversion edge at `current_transform_matrix`; depth counter + `Drop` `debug_assert` | [#231](https://github.com/vanyastaff/flui/pull/231) |
| T8 | `LayerCompositor` — owns `SavedLayer` + `PendingOpacityLayer` + `opacity_stack` + `current_opacity`; `save_layer(&mut GpuStateStack, …)` borrow-enforced; borrows `layer_texture_pool` from `GpuResources` | [#232](https://github.com/vanyastaff/flui/pull/232) |
| T9a–e | `DrawBatcher` record-method relocation: ~3 000 LOC of per-primitive record methods relocated from `painter.rs` into `batches/{shapes,gradients,paths,images}.rs`; batcher sub-modules are `Matrix4`-free (C4 rule, Trigger 19) | branch T9 |
| T9f | Port-check Trigger 19 (C4 grep), ADR-0006, ARCHITECTURE.md record-side boundary section | branch T9 |
| T10a | External-image IR handle: `TextureView`→`TextureId`; resolve at replay, not record | branch T10 |
| T10b | `GpuReplay` introduced; texture_batch scratch + texture-batch flush family moved in | branch T10 |
| T10c | 5 GPU-plumbing fields (viewport_buffer/bind_group, unit_quad×2, default_sampler) + 6 segment-flush families moved into `GpuReplay` | branch T10 |
| T10d | `submit` dispatch loop + `flush_opacity_layer` recursion + `reintegrate_offscreen_content` moved into `GpuReplay::submit`; `WgpuPainter` drops to 1 432 non-test LOC — **C1 closed** | branch T10 |

### What remains (T11)

- **T11 deterministic-replay test:** record an IR, replay to encoder A and encoder B, assert emitted command streams match; compile-time proof record holds no `&mut Device/Queue/Encoder`. This is the remaining C5-gate item.

---

## Decision

### 1. The IR is explicit data, not implicit

`DrawSegment` / `DrawItem` are plain data structs in `command_ir.rs`. Record methods write into them; replay reads them. This is the pattern Impeller used when it deleted its retained EntityPass tree in 2024 ("DisplayList in, flat Command vector out"). The alternative — implicit batching inside `WgpuPainter` with no separation — was the status quo that made every refactor open the full 6 410-line file.

### 2. wgpu stays concrete — no device trait

A `dyn GpuBackend` / device-trait abstraction was explicitly considered and rejected. No second GPU backend exists or is planned. Static dispatch via the single concrete `Backend` impl is the only consumer. A second backend (Skia/Vello/software) would build against a concrete second impl when a real consumer arrives, not against a hypothetical trait today. This is the same decision that deleted `pub trait Painter` in the Mythos chain (Mapping decision §2 in `ARCHITECTURE.md`).

### 3. Borrow-seam pattern — Copy accessors, disjoint borrowed parameters

`GpuStateStack` exposes Copy accessors only — it never returns `&` to internal state, so a caller cannot hold a borrow across the `&mut DrawSegment` write. Each `DrawBatcher` record method takes the narrowest set of disjoint borrowed parameters it needs:

```text
fn draw_rect(
    segment: &mut DrawSegment,
    state: &GpuStateStack,          // Copy accessors only — no &-aliasing hazard
    image: &Image,                  // (example — varies per method)
    …
)
```

`&mut GpuResources` (for `TextureCache` / `ExternalTextureRegistry`) and `opacity: f32` are passed explicitly where needed. No method takes `&mut WgpuPainter` — borrow-splitting enforces the seam.

### 4. Matrix4 conversion at the trait boundary — C4 rule

`GpuStateStack` stores transforms as `glam::Mat4` (glam is the GPU-math crate; it matches the GPU buffer layout directly). The conversion to/from `flui_types::Matrix4` happens at exactly one structural edge: `current_transform_matrix()` in `painter.rs` and the `with_transform` entry in `backend.rs`. The `batches/` submodules, `pipelines.rs`, and `replay.rs` are `Matrix4`-free — enforced by port-check Trigger 19 (added in T9f, extended to replay.rs in T10e). Leaking `Matrix4` into the record/pipeline/replay side would require every GPU-path caller to drag in the flui-types coordinate type and would undo the glam-internal encapsulation that `GpuStateStack` establishes.

### 5. text / rich_text stay painter-side — T9 deferred, T11 seam

`draw_text` and `draw_rich_text` were not relocated to `DrawBatcher` in T9. Rationale: `TextRenderer` is a `glyphon`-owned field (`text_renderer: TextRenderer` on `WgpuPainter`) that holds a GPU glyph atlas — it is not a geometry asset that can be passed as a plain borrowed parameter without restructuring the atlas ownership. The text-vs-IR seam is deferred to T11, where the deterministic-replay proof will clarify whether text commands become IR-opaque handles or are recorded as shaped glyph data. Relocating text before that decision would re-open the record methods a second time.

---

## Alternatives considered

| Option | Why rejected |
|---|---|
| `dyn GpuBackend` device trait | No second backend; premature; every refactor re-types 6 410 lines of audited behavior for a non-existent consumer |
| Keep WgpuPainter as-is (ALT-2) | "No respected Rust renderer resembles it" — the god-object survives; no record/replay, no batching, no testable boundary |
| Separate IR crate (`flui-command-ir`) | Over-splits before a second consumer exists; extractable cheaply once T11 lands |
| immediates / push-constants for per-draw uniforms | Dropped: wgpu-28 renamed, default `max_immediate_size` = 64 B too tight for transform+paint, WebGPU-web pending; market uses instancing + dynamic-offset/storage |

---

## Consequences

**Now (T10 shipped — record/replay split complete):**
- `WgpuPainter` reduced from ~6 410 to **1 432 non-test LOC** (record methods to `batches/`; replay path to `GpuReplay`). **C1 is closed.**
- `GpuReplay` (`replay.rs`) owns the GPU-emit/submit path: 5 GPU-plumbing fields, texture-batch scratch, 6 segment-flush families, `submit` dispatch loop, `flush_opacity_layer` recursion, `reintegrate_offscreen_content`.
- `WgpuPainter` is a thin coordinator: record-finish + `self.replay.submit(...)`. It no longer mixes record and replay concerns.
- `batches/` and `replay.rs` are `Matrix4`-free, enforced by Trigger 19.
- Borrow-seam is structurally enforced; no record method takes `&mut WgpuPainter`.
- Behavior preserved: the five-round wgpu-29 audit net is unchanged (T6 readback safety-net landed before T7).

**After T11:**
- Deterministic-replay test proves the IR separation is non-tautological: record an IR, replay to encoder A and encoder B, assert emitted command streams match.
- Text-vs-IR seam decided; `text` / `rich_text` either join the IR or are documented as a bounded exception.

---

## References

- Spec: `.rust-studio/specs/flui-engine-overhaul/spec.md` — C-IR approach, C4 acceptance criterion, tasks 7–11
- `crates/flui-engine/ARCHITECTURE.md` — wgpu/Vulkan/Metal mapping, mapping decisions §1–6, record-side boundary section (T9f addition)
- `crates/flui-engine/src/wgpu/state_stack.rs` — single `Matrix4`↔glam conversion edge (`current_transform_matrix`)
- `crates/flui-engine/src/wgpu/backend.rs` — `with_transform` + `render_*` entry points; `Matrix4` lives here
- `crates/flui-engine/src/wgpu/batches/` — record-method home; `Matrix4`-free by Trigger 19
- `crates/flui-engine/src/wgpu/replay.rs` — replay/submit home; `Matrix4`-free by Trigger 19 (extended T10e)
- `crates/flui-engine/src/wgpu/painter.rs` — thin coordinator (record-finish + replay.submit); text/rich_text pending T11
- Impeller precedent: DisplayList→flat Command vector, EntityPass tree deleted 2024 (`.flutter/` reference)
- Port-check Trigger 19: `scripts/port-check.sh`; `docs/PORT.md` §19
