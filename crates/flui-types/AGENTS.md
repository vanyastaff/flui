# AGENTS.md — flui-types

Base types crate with **zero flui dependencies**. All geometry, layout, styling, typography, painting, gesture, physics, semantics, and platform types live here (or are re-exported from `flui-geometry`).

## What lives here

- Color, HSLColor, HSVColor (with SIMD paths)
- Layout enums: Axis, Alignment, MainAxisAlignment, CrossAxisAlignment
- Styling: Border, Shadow, Gradient, Decoration
- Typography: TextStyle, TextAlign, TextSpan, InlineSpan
- Painting: BlendMode, BoxFit, Clip, TileMode, Shader
- Gestures: TapDetails, DragDetails, ScaleDetails, Velocity
- Physics: SpringSimulation, FrictionSimulation, Tolerance
- Semantics: SemanticsData, SemanticsAction, SemanticsRole
- Platform: TargetPlatform, Brightness, Locale

Geometry primitives (Point, Rect, Size, Offset, RRect, Matrix4) are **re-exported** from `flui-geometry` via `pub use flui_geometry as geometry`.

## Key constraints

- **No flui crate dependencies** — this is the leaf of the dependency graph (only `flui-geometry` + `smallvec` + optional `serde`)
- Animation types (Curves, Tweens, AnimationStatus) live in `flui-animation`, not here
- Image/font loading lives in `flui-assets`, not here
- `#[must_use]` on most types; const constructors where possible
- Core geometry/layout types are `Copy` (zero heap allocation)

## Testing

```bash
cargo test -p flui-types
cargo bench -p flui-types                  # geometry_bench, color_bench, conversions_bench
```

Property tests use `proptest`; compile-fail tests use `trybuild` (in `tests/compile_fail/`).

## Features

- `serde` — serialization (forwards to `flui-geometry/serde`)
- `simd` — SSE/NEON color paths
- `mint` — interop with glam/nalgebra/cgmath (forwards to `flui-geometry/mint`)
