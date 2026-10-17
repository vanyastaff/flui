### Added

- **`OpenContainer<T>`** (`flui-material`): the Material container transform. A closed surface
  grows into a page and shrinks back, morphing its rect, colour, elevation and corner radii over
  300 ms along `FAST_OUT_SLOW_IN` (the exit runs the curve flipped). `ContainerTransition::Fade`
  dissolves the closed content into the page, `ContainerTransition::FadeThrough` fades through the
  surface colour. The closed surface keeps its size while the page is open, so its neighbours do
  not move; the page closes with a typed result through `CloseAction::close_with`, delivered to
  `on_closed`; and the exit lands on where the source is now, so a list that scrolled meanwhile
  is followed. Under `MotionOverride::Reduce` both directions settle on their first frame.
