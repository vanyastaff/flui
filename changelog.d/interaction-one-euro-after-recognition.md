### Added

- **Post-recognition smoothing** (`flui-interaction`): `GestureSettings::with_position_smoothing`
  selects a `OneEuroTuning` that drag, multi-drag, tap-and-drag, long-press
  moves, force-press updates and the scale focal point apply to the positions they publish after
  acceptance; ends, velocities and native trackpad pan-zoom stay raw. Slop, arena and velocity decisions keep reading raw samples, so
  smoothing never delays or changes which gesture wins; each accepted gesture starts from an
  unsmoothed position, and the last contact sample at one timestamp is the one smoothed. No
  platform preset selects smoothing.

### Changed

- **One Euro tuning is a value** (`flui-interaction`): `OneEuroTuning::new(min_cutoff, beta,
  d_cutoff)` validates parameters and returns `OneEuroFilterError`; `OneEuroFilter::new` and
  `OneEuroFilter2D::new` take a `OneEuroTuning` and no longer fail.
