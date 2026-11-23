### Added

- `SliverGeometry::with_scroll_offset_correction(f64)` requests a scroll offset correction on
  geometry that also carries extents.

### Changed

- `SliverGeometry` is `#[non_exhaustive]`: build it with `SliverGeometry::new`, `with_extents`
  and the `with_*` builders. A struct literal over `SliverGeometry::ZERO` no longer compiles
  outside `flui-rendering`; its omitted fields stayed at zero or `false`, so a sliver could paint
  while taking no layout space, missing hit tests and reporting itself hidden.
- `SliverGeometry::with_visual_overflow` takes the flag (`with_visual_overflow(true)` for the old
  call), so a sliver that computes its overflow sets it with the same builder.
