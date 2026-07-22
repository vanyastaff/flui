# flui-geometry

**Type-safe 2D geometry primitives for the FLUI framework.**

`flui-geometry` is FLUI's foundation geometry crate: points, sizes, rectangles,
transforms, and curves, all parameterized by a compile-time **unit system** so
that logical pixels, physical device pixels, and font-relative units cannot be
mixed by accident.

Part of the [FLUI](https://github.com/vanyastaff/flui) workspace — pre-release,
consumed by path (not published to crates.io).

## Core types

| Type | Description |
|------|-------------|
| `Point<T>` | Absolute position in 2D space |
| `Vec2<T>` | Direction + magnitude (displacement) |
| `Size<T>` | Width/height dimensions |
| `Offset<T>` | UI displacement (Flutter-compatible) |
| `Rect` / `RRect` / `RSuperellipse` | Axis-aligned, rounded, and superellipse rectangles |
| `Circle` / `Line` / `Bezier` | Curve and shape primitives |
| `Matrix4` / `Transform2D` | 4×4 and 2D affine transforms (glam-backed) |
| `RelativeRect` / `QuarterTurns` | Parent-relative positioning and 90° rotation steps |

## Unit system

| Unit | Meaning |
|------|---------|
| `Pixels` | Logical pixels — UI layout and design coordinates |
| `DevicePixels` | Physical pixels (integer) — framebuffer addressing |
| `Rems` | Root-em, font-relative sizing |

Conversions between unit spaces are explicit (`ScaleFactor`), and `From<f32>`
escape hatches are deliberately absent — the unit barrier is enforced by the
workspace's port-check CI gate.

```rust
use flui_geometry::{Point, Size, Rect, px};

let origin = Point::new(px(10.0), px(20.0));
let size = Size::new(px(100.0), px(50.0));
let rect = Rect::from_origin_size(origin, size);
assert_eq!(rect.center().x, px(60.0));
```

## Design notes

- **Flutter parity, Rust shape.** The observable behavior (edge cases,
  ordering, coordinate conventions) follows Flutter's `dart:ui` geometry;
  the structure is Rust-native (`Copy` value types, `glam` SIMD backing for
  `Matrix4`, `bytemuck` Pod derives for zero-copy GPU upload).
- **No heap allocations** in the core types; everything is stack-`Copy`.
- **Interop:** optional `mint` (math ecosystem), `kurbo` (Bézier bridge for
  the painting layer), and `serde` support behind feature flags.

## Documentation

Every public item is documented (`#![deny(missing_docs)]`); build locally with
`cargo doc -p flui-geometry --open`. Architecture context lives in the
workspace's [`docs/FOUNDATIONS.md`](../../docs/FOUNDATIONS.md).

## License

MIT OR Apache-2.0, per the workspace license.
