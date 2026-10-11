### Changed

- Remove the unused `recognizers::constants` defaults; use the typed gesture settings and their canonical `DEFAULT_*` values. Drag aliases name one recognizer type; the `vertical_drag`, `horizontal_drag` and `pan` constructors select its axis.

### Fixed

- Deliver accepted standalone hover and sequence boundaries even after `Leave`; interpolated positions retain the previous measured sensor reading.
- Process force-press pressure and motion in measured packet order, ignoring predictions and stopping stale history after contact replacement. Missing pressure preserves the last measured pressure coordinates.
