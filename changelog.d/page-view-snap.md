### Added

- **`AnchorSnap`**: a `SnapPolicy` over a set of offsets (sheet stops, revealed row actions,
  uneven item starts) for `Scrollable::snap_policy`; `AnchorSnap::new` refuses an empty or
  non-finite set with `AnchorSnapError`.
- **`PageView` keys and devices**: the scroll keys turn pages (an arrow toward the next page,
  PageDown/PageUp, Space/Shift+Space, Home and End), `PageView::autofocus`,
  `PageView::allowed_devices` and `Scrollable::allowed_devices` (drags and presses of other pointer
  kinds are refused), `Scrollable::semantic_label`; assistive technology reads
  `WidgetsLocalizations::page_label` ("Page N of M" by default).

### Changed

- **`PageView` is a `Scrollable` with a `PageSnap` policy**; a release, a key and an assistive
  scroll settle on a page through the snap policy. `PageSnap` takes the next page in a fling's
  direction from an exact page offset too, and counts half a device pixel off a page as on it.

### Removed

- **`PageScrollPhysics`**: use `Scrollable::snap_policy` with `PageSnap`.
