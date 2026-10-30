### Added

- `flui_material::CarouselView`: Material's carousel, a `Carousel` whose items are M3 cards that
  cover the shown part of each item with rounded corners while the content keeps its full extent.
  `on_tap` makes each card a button with a ripple. Styled by `CarouselViewThemeData`
  (`ThemeData::carousel_view_theme`) with the M3 defaults. Its snap follows the layout unless
  `snap` sets it, `snap(None)` scrolling freely.
- `KeylineSnap::for_layout`: Material's snap for a layout, one item per swipe for every layout
  but an uncontained one.
- `CarouselLayout::kind` and `CarouselLayoutKind` in `flui-objects`, re-exported by
  `flui-widgets`.
