### Added

- `Carousel` in `flui-widgets`: a horizontal, lazily built carousel on Material's keyline model.
  Items are laid out at the large extent and masked down to the keylines they pass, read their
  `CarouselItemScope` (mask and parallax progress), and follow the reading direction. Layouts are
  checked when made (`CarouselLayout::multi_browse`, `uncontained`, `hero`, `full_screen`,
  `weighted`, returning `CarouselLayoutError`).
- `KeylineSnap` settles a release on an item: `SingleAdvance` one item from where the drag began,
  `MultiBrowse` where the fling coasts.
- `CarouselController`: the active item, `animate_to_item`, `jump_to_item` and
  `on_index_changed`, delivered after the frame; one mounted carousel per controller.
- `CarouselAutoplay::every` (at least five seconds) with a pause control the carousel draws, and
  pauses on focus, pointer, hover and the app leaving the front.
- `SnapPolicy::target_from`: a snap policy may read where the drag began; it defaults to
  `target`.
- `RenderSliverCarousel`, `CarouselGeometry`, `CarouselConfig` and `CarouselItemFrames` in
  `flui-objects`, and `SliverCarousel` in `flui-view`.

### Changed

- `WidgetsLocalizations` requires `autoplay_pause_label` and `autoplay_resume_label`, the labels
  of a carousel's autoplay control.
