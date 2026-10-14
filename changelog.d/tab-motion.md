### Added

- **Tab position and animated indicator** (`flui-material`): `TabController::position()` returns a
  `TabPosition`, the continuous fractional tab position, derived from one source: the
  controller's own animated value when no `TabBarView` is attached, the view's pager while one is.
  `TabController::with_motion` and `DefaultTabController`'s `MotionBuilder` configure the tab-change
  motion (300 ms ease-in-out by default); reduced motion settles it on the next frame. The
  `TabBar` indicator follows the position, sliding (`TabIndicatorAnimation::Linear`, the default)
  or stretching (`TabIndicatorAnimation::Elastic`) between tabs, and mirrors under right-to-left
  text.

### Changed

- **`TabBarView` is a swipeable pager** (`flui-material`): it composes a `PageView`, so pages can
  be dragged; the selected index commits once the pager comes to rest, not during the drag. Every
  page is built up front and stays mounted (previously a page was built on first visit and hidden
  pages had their tickers muted). A controller drives at most one mounted `TabBarView`; a second
  view on the same controller fails its build with an error view.
- **`TabController::animate_to` animates** (`flui-material`): it commits the index at once and
  animates the position; `set_index` jumps.
