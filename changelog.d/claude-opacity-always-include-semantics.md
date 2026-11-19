### Added

- **`flui-widgets`**: `Opacity::always_include_semantics` and
  `SliverOpacity::always_include_semantics` keep a fully transparent child's semantics
  published. By default a child at opacity `0.0` is hidden from assistive technology, as it is
  from sight. `RenderOpacity` and `RenderSliverOpacity` carry the flag with its getter and
  setter.
