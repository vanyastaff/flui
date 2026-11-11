### Added

- `Scrollbar` follows the scroll view nearest below it without a shared controller; a list nested
  inside that one neither replaces nor moves its bar. `Scrollbar::controller(controller,
  axis_direction)` names a scroll view beside the bar instead.
- `Scrollbar` works on both axes, under RTL and on reversed lists, and shrinks its thumb with the
  overscroll. It takes a thumb drag and a track tap from any device (a tap pages by 0.8 of the
  viewport) and a wheel tick over it, and fades out after the scroll stops (`fade_duration`,
  `time_to_fade`; at once under reduced motion).
- `Scrollbar` knobs `thumb_visibility`, `track_visibility`, `interactive`, `thickness`, `radius`,
  `main_axis_margin`, `cross_axis_margin`, `min_thumb_length`, `thumb_color`, `track_color` and
  `orientation` (`ScrollbarOrientation`).
- `ScrollBehavior::build_scrollbar` and `ScrollableDetails`: a behavior adds a bar to each scroll
  view under it, with `Scrollbar::for_scrollable`.

### Changed

- `Scrollbar::new` takes its child; `child` and `thumb_width` are gone (`thickness` replaces the
  latter). A scroll frame repaints the bar and rebuilds nothing.

### Removed

- `ScrollController::thumb_fraction` and `ScrollController::thumb_offset_fraction`: the bar's
  geometry is its own.
