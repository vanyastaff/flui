### Added

- **`flui-material` ripple**: `InkWell` paints a Material 3 state layer (hover 0.08, focus 0.10,
  pressed 0.10, dragged 0.16, animated 15/45/150 ms) and a press ripple (75 ms fade-in, 225 ms
  radius, 150 ms fade-out; a quick release finishes its fade-in first) under its child, clipped
  to its shape. `RippleScope` provides an ambient `RippleConfiguration { color, alpha }`, and
  `RippleScope::new(None, ..)` turns the feedback off. Reduced motion keeps the state layer and
  drops the expanding wave.
- **`flui-sdk`**: `painting::ClipOp`.

### Changed

- **`flui-material`**: an `InkWell` whose `overlay_color` resolves to `None` now falls back to
  the ambient ripple colour (else the theme's `on_surface`) at the M3 state-layer alpha instead of
  painting nothing; component overlay tables read their alphas from `StateLayerAlpha::M3`.
