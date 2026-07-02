---
title: "Scene / DrawCommand contract freeze"
status: frozen
contract_version: 1
date: 2026-06-30
roadmap: Core.0 N11
guards:
  - "crates/flui-painting/src/display_list/command.rs — `mod contract_freeze` (exhaustive-match compile guard + count assertion)"
---

[← Designs index](.) · [Roadmap](../ROADMAP.md) · [Tracker (N11)](../ROADMAP-TRACKER.md)

# Scene / DrawCommand contract freeze (Core.0 N11)

> **Why this exists.** The engine track (`flui-engine`, its wgpu backend, future
> Vello/software backends) and the painting track (`flui-painting` `Canvas` →
> `DisplayList`) are developed in parallel. They meet at exactly one data
> contract: the **`DrawCommand` stream** a `DisplayList` carries, composited into
> a **`Scene`** (`flui-layer`) and replayed by a backend through
> `CommandRenderer`. If that contract drifts silently, every backend and every
> producer can diverge undetected. This document **freezes** the contract and
> records the CI guard that makes a change to it impossible to land silently.

---

## The contract surface

### 1. `DrawCommand` — the wire format (FROZEN)

Defined in [`crates/flui-painting/src/display_list/command.rs`](../../crates/flui-painting/src/display_list/command.rs),
`#[non_exhaustive]`, **31 variants** at contract version 1:

| Group | Variants |
|---|---|
| Shapes | `DrawLine` · `DrawRect` · `DrawRRect` · `DrawCircle` · `DrawOval` · `DrawPath` · `DrawArc` · `DrawDRRect` · `DrawPoints` · `DrawVertices` |
| Text | `DrawText` · `DrawTextSpan` |
| Images | `DrawImage` · `DrawImageRepeat` · `DrawImageNineSlice` · `DrawImageFiltered` · `DrawTexture` · `DrawAtlas` |
| Fills / effects | `DrawColor` · `DrawPaint` · `DrawShadow` · `DrawGradient` · `DrawGradientRRect` · `ShaderMask` · `BackdropFilter` |
| Clipping | `ClipRect` · `ClipRRect` · `ClipRSuperellipse` · `ClipPath` |
| Layers | `SaveLayer` · `RestoreLayer` |

Every variant carries a `transform: Matrix4` (record-time transform) so the
command stream is position-independent and replayable. `Paint` is interned
behind `Arc<Paint>` on the variants that take it.

### 2. `DisplayList` — the command container

[`display_list/mod.rs`](../../crates/flui-painting/src/display_list/mod.rs):
fields (`commands: Vec<DrawCommand>`, `bounds: Rect<Pixels>`) are **`pub(crate)`**
— the public surface is the constructor + accessor methods (`commands()`,
`bounds()`), not the fields. Adding a field is therefore non-breaking; the
encapsulation is the stability guarantee.

### 3. `Scene` — the composited frame

[`flui-layer/src/scene.rs`](../../crates/flui-layer/src/scene.rs): all fields are
**private** (`size`, `layer_tree`, `root`, `link_registry`,
`composition_callbacks`, `frame_number`); the surface is its constructors
(`empty`, `new`, `from_layer`) and accessors. The engine consumes a `&Scene`
through `Renderer::render_scene` / the `RasterBackend` seam (see the companion
[RasterBackend seam design](2026-06-30-rasterbackend-seam.md)).

### 4. Consumer side — `CommandRenderer`

[`flui-engine/src/traits.rs`](../../crates/flui-engine/src/traits.rs): one
`render_*` method per `DrawCommand` variant; [`commands.rs`](../../crates/flui-engine/src/commands.rs)
`dispatch_command` routes each variant to its method. A backend that omits a
variant is caught by the `_ => warn!` arm at runtime — the freeze guard below
catches it at **build** time on the producer side.

---

## The freeze guard (CI tripwire)

The guard lives in the **defining crate** so it can match the
`#[non_exhaustive]` enum exhaustively (`#[non_exhaustive]` is a no-op within the
defining crate):

[`command.rs` → `mod contract_freeze`](../../crates/flui-painting/src/display_list/command.rs):
- `contract_discriminant(&DrawCommand)` is an **exhaustive match with no
  wildcard arm**. Add a variant → `error: non-exhaustive patterns`. Remove or
  rename one → `error: no variant named …`. The contract cannot change without
  breaking the build.
- `FROZEN_DRAWCOMMAND_VARIANT_COUNT = 31` + `drawcommand_contract_is_frozen()`
  pin the count as a second, human-readable signal.

This runs in the normal `cargo test`/`cargo nextest` gate — no separate tooling,
no fragile grep. It is strictly stronger than a `cargo public-api` snapshot for
this enum because it is a *compile* error, not a diff.

---

## Change protocol

A change to `DrawCommand` is a **coordinated cross-track change**, never a local
edit. To change the contract:

1. **Bump** `contract_version` in this document's frontmatter and add a line to
   the changelog below explaining the variant added/removed/renamed and why.
2. **Producer:** add/rename the variant in `command.rs`, update the
   `contract_freeze` match arm and `FROZEN_DRAWCOMMAND_VARIANT_COUNT`, and wire
   the `Canvas` builder method that emits it.
3. **Consumer:** add the matching `render_*` method to `CommandRenderer`
   (`traits.rs`) and the dispatch arm in `commands.rs`. Every backend
   (`WgpuPainter`, `DebugBackend`, and any future Vello/software backend) must
   implement it or the engine fails to compile.
4. **Re-run the full gate** (`cargo nextest run --workspace`, `port-check.sh`)
   so a backend missing the new arm fails loudly.

Additive change (a new variant) is API-compatible for downstream crates thanks
to `#[non_exhaustive]`; removal/rename is breaking and requires the version bump
to be a major contract revision.

---

## Changelog

| Contract version | Date | Change |
|---|---|---|
| 1 | 2026-06-30 | Initial freeze at 31 `DrawCommand` variants. Guard installed. |
