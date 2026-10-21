### Changed

- **`flui-widgets`**: `PageView::new` takes the pager's `GestureAlternatives`, whose commands
  name the new `PageIntent::{Next, Previous}`; the pager turns one page for each, so a page
  turns without a swipe by its controls, its custom actions and their keys
  ([ADR-0216](/docs/adr/ADR-0216-gesture-alternatives.md)).
- **`flui-widgets`**: `AnchoredDrag::new` takes the drag's `GestureAlternatives`, whose
  commands name the new `AnchorIntent::{Next, Previous}`. Their row is the drag's one stop of
  focus traversal, named or not, and the arrow keys along the axis move it there;
  `AnchoredDrag::semantics` now names the slider only.

### Fixed

- **`flui-widgets`**: a wheel notch, a key press or an assistive scroll on a list stops the
  fling it handed to a scrollable above it, as a touch already did; the fling's sequence ends
  `Preempted` before the new one starts
  ([ADR-0209](/docs/adr/ADR-0209-nested-scroll-connection.md)).
