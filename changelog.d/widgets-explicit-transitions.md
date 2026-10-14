### Added

- **`flui-widgets`**: `SizeTransition`, `AlignTransition`, `PositionedTransition` and
  `DecoratedBoxTransition`, driven by an `Animation` and updated per tick by their render object
  without rebuilding the element tree.
- **`flui-objects`**: `RenderAnimatedAlign` with `AlignMotion`, `RenderAnimatedInset` and
  `RenderAnimatedDecoratedBox`, the render objects behind those transitions.
- **`flui-painting`**: `Lerp` for `BoxDecoration<f64>`, so `Tween<BoxDecoration<f64>>` animates a
  decoration's color, border, radius, shadows and gradient together.
- **`flui-foundation`**: `RelativeRect` is `Clone`, `Copy` and `PartialEq` and implements `Lerp`.
- **`flui-sdk`**: `painting::BoxShadow`.

### Changed

- **`flui-cupertino`**: `cupertino_page_route` casts an edge shadow off the moving page while it
  slides; a settled page paints none.
