### Added

- **`flui-widgets`**: `Align::new`, `Stack::alignment` and `IndexedStack::alignment` accept an
  `AlignmentDirectional` (via `impl Into<AlignmentGeometry>`), resolved against the ambient
  `Directionality` inside the render object — so an RTL flip re-lays them out. `From<Alignment>`
  keeps every `Alignment` call site compiling.

### Removed

- **`flui-widgets`**: `resolve_alignment`, the build-side seam that resolved a directional
  alignment before handing it to a render-object widget. `Align`, `Stack` and `IndexedStack` now
  resolve direction themselves through `RenderObjectContext::depend_on`, so the seam has no
  remaining users.
