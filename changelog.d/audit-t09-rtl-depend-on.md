### Added

- **`flui-widgets`**: `Align::new`, `Stack::alignment` and `IndexedStack::alignment` accept an
  `AlignmentDirectional` (via `impl Into<AlignmentGeometry>`), resolved against the ambient
  `Directionality` inside the render object — so an RTL flip re-lays them out. `From<Alignment>`
  keeps every `Alignment` call site compiling.

### Fixed

- **`flui-widgets`**: a vertical `Viewport`/`ShrinkWrappingViewport` under right-to-left text now
  lays its horizontal cross axis right-to-left (previously hardcoded left-to-right). The render
  objects gain `set_cross_axis_direction`/`cross_axis_direction` and the widgets resolve the
  direction from the ambient `Directionality`.
- **`flui-widgets`**: `IconData::match_text_direction` now mirrors the glyph — `Icon` composes a
  `Transform` about the glyph's centre under right-to-left text, where previously the flag was
  documented as deferred.

### Removed

- **`flui-widgets`**: `resolve_alignment`, the build-side seam that resolved a directional
  alignment before handing it to a render-object widget. `Align`, `Stack` and `IndexedStack` now
  resolve direction themselves through `RenderObjectContext::depend_on`, so the seam has no
  remaining users.
