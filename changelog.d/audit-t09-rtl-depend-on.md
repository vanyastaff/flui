### Added

- **`flui-view`**: `RenderObjectContext::depend_on::<I>()` reads the nearest `InheritedView` from
  `RenderView::update_render_object` and records a dependency, so the render object is
  reconfigured when that provider changes. It returns `None` in `create_render_object`; the
  element's first build pushes the resolved configuration before the first layout.
- **`flui-widgets`**: `Align::new`, `Stack::alignment` and `IndexedStack::alignment` accept an
  `AlignmentDirectional` (via `impl Into<AlignmentGeometry>`), resolved against the ambient
  `Directionality` inside the render object — so an RTL flip re-lays them out. `From<Alignment>`
  keeps every `Alignment` call site compiling.
- **`flui-widgets`**: `Positioned::directional(child)` returns a `DirectionalPositioned` that
  places a child by `start`/`end` (and `top`/`bottom`/`width`/`height`), resolved against the
  ambient `Directionality` (`start` is left in LTR, right in RTL). `Positioned` keeps
  `left`/`right` only, so one child cannot name both pairs.
- **`flui-rendering`**: `HorizontalInsets`, a stack child's horizontal pair, either
  `Physical { left, right }` or `Directional { start, end }`, with `resolve(TextDirection)`.
- **`flui-objects`**: `RenderStack`/`RenderIndexedStack` gain `with_text_direction` and
  `set_text_direction`; `RenderViewport`/`RenderShrinkWrappingViewport` gain
  `set_cross_axis_direction`; `RenderIcon` gains `with_mirrored`/`set_mirrored`.

### Changed

- **`flui-view`**: `RenderView::update_render_object` runs from the render element's build, once
  per rebuild (after a view swap, a dependency change, and on the first build), not from the view
  swap itself. A panic in it now replaces the element with the error view through the same
  containment as `build`, recorded once with `LifecycleHook::Update`.
- **`flui-rendering`**: `StackParentData`'s `left` and `right` fields are replaced by
  `horizontal: HorizontalInsets`; `with_left`/`with_right` still set the physical pair, and
  `with_horizontal` sets either pair.
- **`flui-objects`**: `PositionedSpec::from_parent_data` takes the stack's `TextDirection` as a
  second argument and resolves directional insets against it.
- **`flui-widgets`**: `Flex`, `Row`, `Column`, `Wrap` and `ListBody` are render views reading
  `Directionality` themselves; they no longer implement `StatelessView`, and their private
  render-view shims are gone.

### Fixed

- **`flui-widgets`**: a vertical `Viewport`/`ShrinkWrappingViewport` under right-to-left text now
  lays its horizontal cross axis right-to-left (previously hardcoded left-to-right).
- **`flui-widgets`**: `IconData::match_text_direction` now mirrors the glyph — `Icon` resolves the
  ambient `Directionality` and `RenderIcon` flips the glyph about the box centre in paint, where
  previously the flag was documented as deferred.

### Removed

- **`flui-widgets`**: `resolve_alignment`, the build-side seam that resolved a directional
  alignment before handing it to a render-object widget. `Align`, `Stack` and `IndexedStack` now
  resolve direction themselves through `RenderObjectContext::depend_on`, so the seam has no
  remaining users.
