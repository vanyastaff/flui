# ADR-0057 — Coverage-correct blending is capability-gated, and the fallback is named

- **Status:** Accepted
- **Date:** 2026-09-05
- **Issue:** #904
- **Supersedes:** nothing. Records a rendering contract that
  `crates/flui-engine/ARCHITECTURE.md` mapping decision 7 implements.
- **Depends on:** `wgpu::Features::DUAL_SOURCE_BLENDING`, which is optional and
  absent on WebGPU.

## Context

An anti-aliased clip could not feather a destructive blend's edge. Shaders fold
clip coverage into the source alpha, so the blender computes
`S(a·cov)·(src·cov) + D(a·cov)·dst` where the coverage-correct answer is
`lerp(dst, S(a)·src + D(a)·dst, cov)`.

Those agree **iff the mode's destination factor `D` absorbs the `(1−cov)`
term** — `D` of the form `1 − k·srcAlpha`, or `D = 1`. That is a property of the
factor pair, not of any one mode. Seven modes fail it: `Clear`, `Src`, `SrcIn`,
`DstIn`, `SrcOut`, `DstATop`, `Modulate`. All seven were confirmed wrong on
hardware.

The same partition already existed in the tree. `is_tile_safe_for_ssaa`'s
coverage-destructive exception set is character-for-character that list, because
"does `D` equal 1 at source alpha 0" and "does `D` absorb `1−cov`" are the same
question asked for different reasons.

## Decision

Coverage gets its own blend channel. The fragment shader emits
`src1 = coverage · (1 − k·alpha)` as a second blend source and the seven modes
take `dst_factor = OneMinusSrc1`, where `k` is the destination factor expressed
as a scalar multiple of source alpha (`0` or `1`).

**This requires `DUAL_SOURCE_BLENDING`, which WebGPU does not expose and this
workspace ships a wasm32 target for. The corrected path is therefore
capability-gated, and on a device without the feature the folded — wrong —
behaviour stands.**

That is the part this ADR exists to ratify. The same layer tree produces
feathered edges on a capable native backend and hard ones on WebGPU. It is an
observable cross-platform rendering difference, not an internal representation
choice, so it is recorded at protocol level rather than only in a crate's
mapping decisions.

## Why this is acceptable

- **The alternative is worse everywhere.** Without the gate, all seven modes
  stay wrong on every backend, including the ones that can do better. A
  divergence that makes capable devices correct is preferable to uniform
  incorrectness.
- **The difference is bounded.** It appears only in partial-coverage pixels —
  the fractional edge of an anti-aliased clip — of seven blend modes. Fully
  covered and fully excluded pixels are identical on both paths, which the
  oracles assert directly.
- **The fallback is the status quo ante**, not a new behaviour. Nothing that
  renders correctly today renders differently on a device without the feature.
- **It is testable on both paths.** `HeadlessRenderer::without_dual_source_blending`
  builds a device with the feature withheld, and every oracle renders both ways
  in one test — the corrected half asserting the feathered value, the fallback
  half asserting the folded one. The divergence is pinned, not merely described.

## What was rejected

- **`(Zero, OneMinusSrcAlpha)` for `Clear`.** The mechanical fix, and it forces
  a behavioural choice this design avoids: with coverage and paint alpha sharing
  one channel, a translucent `Clear` paint would begin to half-erase. Skia keeps
  them independent; conflating them means picking which meaning to break.
- **Correcting every mode unconditionally.** Requiring the feature would break
  the wasm32 target outright.
- **Leaving it uncorrected until WebGPU gains the feature.** Indefinite, and it
  keeps a visible defect on the backends most users run.

## Consequences

- A pixel comparison across backends must account for this. Any future
  cross-backend golden-image test needs to know which path produced it.
- If WebGPU gains dual-source blending, the gate collapses and this ADR is
  superseded by its removal rather than amended.
- New blend modes must be classified against the absorbing rule. Adding one
  without deciding whether its `D` absorbs `1−cov` reintroduces the defect
  silently for that mode.
