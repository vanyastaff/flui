### Added

- **`flui_view::StatefulRenderView` and `RenderViewState`**: a view whose one element holds a
  state and a render object. The view creates the object; the state configures it at every
  build, publishes values to the subtree and builds the child (ADR-0322 §4).
  `impl_stateful_render_view!` writes the `View` implementation.
