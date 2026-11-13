### Added

- `PageTransitionBuilder::back_gesture` and `PageTransitionBuilder::barrier_color`: a transition
  builder carries the page's chrome, and a route that adopts the builder adopts it unless the
  route sets its own. `ParallaxPageTransition` pops its page on a drag from the leading edge, and
  `ParallaxPageTransition::barrier` sets the dim under the page.

### Changed

- A page under `CupertinoApp` pops on an edge swipe and dims the page below while it slides over
  it, as a `CupertinoPageRoute` does; a page under Material on Apple platforms pops on the swipe
  with no dim, as a `MaterialPageRoute` does.
- A route's barrier colour fades in along its entrance and out along its exit, and paints nothing
  under an opaque page that has entered.
