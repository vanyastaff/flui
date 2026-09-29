### Changed

- Logical lengths are plain `f64` everywhere, including scroll offsets and the sliver protocol: `10.0` is ten logical pixels. Write `Size::new(100.0, 50.0)` where you wrote `Size::new(px(100.0), px(50.0))`, and `Size` where you wrote `Size<Pixels>` ([ADR-0098](/docs/adr/ADR-0098-owned-f64-geometry-values.md)).
- Geometry values (`Point`, `Offset`, `Size`, `Rect`, `RRect`, `EdgeInsets`, `Matrix4`, `Axis`, and the device-grid `DevicePoint`, `DeviceSize`, `DeviceRect`) live in `flui_foundation::geometry`, reachable as `flui::geometry` and `flui_sdk::geometry`.
- Paint, style and text values live in `flui-painting`: `flui::painting::{paint, styling, typography}`, plus `Alignment`, `BoxFit`, `BoxShape` and `TextBaseline`. Platform values (`Brightness`, `Locale`, `TargetPlatform`, `ImeEvent`, `HapticFeedback`) live in `flui-platform-api`, reachable as `flui::platform`; gesture details and `Velocity` in `flui-interaction`; `AxisDirection` and `TableCellVerticalAlignment` in `flui-rendering`; the flex, stack, wrap and table-column enums in `flui-objects`.
- `Color::lerp` interpolates premultiplied, so a fade to transparent keeps its hue instead of darkening; between opaque colours the result is unchanged.
- `Path` is backed by kurbo: containment uses the exact winding number, bounds are tight, and rectangles, ovals and arcs are curves. `Path::commands()` returns an iterator of `MoveTo`, `LineTo`, `QuadraticTo`, `CubicTo` and `Close`.
- A hard-edged rectangular clip keeps exactly the pixels whose centres are inside it, and the damage region, the scissor in front of a rounded clip, and backdrop and blend copy regions cover every pixel they touch.
- Geometry value types are no longer `Eq` or `Hash`; `Color` still is.

### Added

- `flui_foundation::geometry::{snap, snap_edges, cover, device_rect_covering, device_size, resolve_stroke_width}` and `DevicePixelRatio`: one rounding rule from logical coordinates to the device grid.

### Removed

- The `flui-types` and `flui-geometry` crates, with `flui::types` and `flui_sdk::types`.
- `Pixels`, `DevicePixels`, `PixelDelta`, `Rems`, `Percentage`, `ScaleFactor`, `Radians`, the `Length` family, `px()` and `device_px()`.
- Values with no consumer: `PointerData`, `OffsetPair`, `DeviceOrientation`, `FractionalOffset`, `Orientation`, `MaterialColors`, the Bézier and text-path types, `Brightness`'s colour helpers, the `*_radians` method duplicates, and `flui-types`' copies of `BoxConstraints`, `FlexFit`, `CacheExtentStyle` and the physics simulations.

### Fixed

- A 100 ms animation run started off a whole-millisecond frame completes on its last frame; the elapsed time is taken on the clock's integer grid, so it no longer lands one ulp short.
