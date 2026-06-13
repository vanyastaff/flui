# AGENTS.md — flui-geometry

Type-safe 2D geometry primitives with compile-time unit safety. Split from `flui-types` in D-block PR-C-2.

## What lives here

- `Point<T>`, `Vec2<T>`, `Size<T>`, `Offset<T>` — generic over unit type
- `Rect`, `RRect`, `Circle`, `Line` — concrete geometry shapes
- `Matrix4` — 4×4 transformation matrix (delegates to `glam`)
- Unit types: `Pixels`, `DevicePixels`, `Rems`
- Constructor helpers: `px()`, `device_px()`, `rem()`

## Key constraints

- **No `From<f32>` for unit wrappers** — enforced by port-check trigger #14. Use `px()` / `::new()` instead
- `glam` is the linear-algebra backend (non-optional); `bytemuck` for GPU upload
- `Matrix4` delegates to `glam::Mat4`; `Vec2` delegates to `glam::Vec2`
- Epsilon comparisons are done at higher levels, not here — `float_cmp` is allowed in lints

## Features

- `serde` — serialization for geometry types
- `mint` — interop with glam/nalgebra/cgmath
- `kurbo` — f64 Bezier/curve math bridge (gated, for painting layer)
