### Fixed

- A `Scrollable` rebuilt during a fling keeps it: a fresh physics value (`.physics(Arc::new(..))`
  in a build) is taken in place instead of replacing the scroll owner, and a change that does
  replace it (a new `NestedScroll` connection above, a new axis or overscroll behavior) hands
  the run, the contact, a held key and a drop autoscroll to the new owner. The list no longer
  freezes mid-flight or stays past its edge, and a tap after it rests reaches the content.
- `ScrollController::jump_to(f64::NAN)` no longer panics inside a participant's clamp: a scroll
  step that is not finite is dropped where it enters the nested scroll channel.
- A remainder or a drop autoscroll step that reaches a scrollable past its edge moves it toward
  its range by the step, instead of snapping it onto the edge.
- A horizontal `RefreshIndicator` pulls from the reading start: from the right edge under
  right-to-left text, with its indicator shown there, and an overscroll at the list's end no
  longer refreshes.
- A nested fling hands its residual on only when its run came within half a device pixel of the
  edge, on lists of any length.
- `PageSnap` on a page shorter than a logical pixel leaves a release where it is instead of
  jumping it back.

### Changed

- `PageController::with_params` and `PageSnap::new` take a `ViewportFraction`, built with
  `ViewportFraction::new` (or `TryFrom<f64>`), which refuses a NaN, infinite, zero or negative
  share with `InvalidViewportFraction`; an infinite fraction no longer panics a `PageView` at
  mount. `ViewportFraction::FULL` is a page the viewport's length.
