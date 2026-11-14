### Added

- `Tester::is_visible` and `Tester::try_is_visible`: whether an element a locator matches lies inside the window and every clipping ancestor, the `visible` check an action makes.

### Fixed

- A list's rows past its viewport are off screen for assistive technology, as Flutter marks them `isHidden`: the node that covers its children's paint clip publishes AccessKit's `clips_children`, so a screen reader reaches the row in view and the first row past each edge (which it moves to in order to scroll it in), not the rest of the cache. The agent and `Tester` still find every built row, as `flutter_test`'s `find` does.
- The agent's `visible` check, and so `Tester::tap` and the other actions, no longer counts a row past a viewport as visible when the window shows the spot it lies on: a tap on it fails with `NotActionable` (`visible`).
