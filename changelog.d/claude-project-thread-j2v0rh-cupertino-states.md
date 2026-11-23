### Added

- **`CupertinoActivityIndicator` takes a radius, a colour and a stop** (`flui-cupertino`):
  `radius` sizes it as a square of twice the radius (an invalid radius keeps the default 10),
  `color` tints its ticks, and `animating(false)` paints the ticks at rest and requests no frames;
  changing it on a mounted indicator starts or stops the cycle.
  `CupertinoActivityIndicator::partially_revealed(progress)` shows that fraction of the ticks at
  one alpha, never animates, and announces its progress as a percentage; a progress outside
  `0..=1` is clamped and NaN reveals nothing.

### Changed

- **`CupertinoActivityIndicator` is no longer a unit struct** (`flui-cupertino`): build it with
  `CupertinoActivityIndicator::new()` or `Default` instead of `CupertinoActivityIndicator {}`.
