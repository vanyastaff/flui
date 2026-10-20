### Changed

- **`ScrollController::update_dimensions` returns `Result<(), InvalidExtents>`** (`flui-widgets`):
  a viewport length or extent that is not finite, a negative viewport or `min > max` is refused
  and the position keeps its extents, instead of panicking later in a clamp. `InvalidExtents` is
  exported from `flui_widgets`.
- **`BouncingScrollPhysics` stiffens with the overscroll** (`flui-widgets`): the drag resistance
  is `overscroll_spring_coefficient · (1 − overscroll / viewport)²`, integrated over the pull, so
  overscroll stays within the viewport's length and a pull split over many events lands where one
  would. A 1200 px pull at the edge of a 600 px viewport now moves the content about 306 px out,
  not 624.
- **A click during a notched wheel's easing reaches the content** (`flui-widgets`): a press no
  longer catches a wheel's easing, which goes on under the click; a fling, snap, settle or
  programmatic animation is still caught.

### Fixed

- **A fling rested in blank space past a lazy list's end** (`flui-widgets`): a release's run is
  rebuilt from the shown offset at its current velocity when layout commits other content extents
  under it, so a list whose rows measure shorter than its estimate stops (or bounces back) on its
  real end. `ClampingScrollPhysics` returns a release from past an edge to it on a critically
  damped spring instead of refusing a run there.
- **Unordered or NaN extents panicked** (`flui-widgets`, `flui-rendering`): `ScrollPosition`
  refuses such content extents from layout, and `ClampingScrollPhysics`,
  `BouncingScrollPhysics` and `ScrollMetrics::page` clamp nothing for them.
- **A release taken over by reentrant code left its scroll sequence open** (`flui-widgets`): when
  an activity listener or a physics moved the offset while a release's run was starting, the
  sequence ended only at the next contact, as `Cancelled`; it now ends at once, `Released`.
