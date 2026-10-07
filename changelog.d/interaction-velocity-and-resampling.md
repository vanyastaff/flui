### Added

- **`VelocityTracker::estimate_at` / `velocity_at`** (`flui-interaction`): query a velocity as of a
  time on the samples' own clock, so the 40 ms "pointer stopped" gate is deterministic under
  virtual clocks and replay.
- **`PointerEventResampler::add_event_at`** (`flui-interaction`): queue an event at an explicit
  time on the sampling clock.

### Changed

- **`GestureSettings`** (`flui-interaction`) is validated: `new` is replaced by `try_new`, the
  `f64` builders by `try_with_touch_slop`/`try_with_pan_slop`/`try_with_pan_slop_vertical`/
  `try_with_pan_slop_horizontal`/`try_with_scale_slop`/`try_with_double_tap_slop`, and
  `with_min_fling_velocity`/`with_max_fling_velocity` by `try_with_fling_velocity(min, max)`.
  They return `GestureSettingsError` for NaN, infinite or negative values and for `min > max`.
- **`GestureSettings::clamp_fling_velocity`** (`flui-interaction`) clamps the magnitude to the
  maximum fling velocity and keeps the sign; it no longer raises slow velocities to the minimum
  (which flipped negative velocities) and never panics.
- **`PointerEventResampler`** (`flui-interaction`) places events by their own time, interpolates
  at `(sample - last) / (next - last)`, and coalesces moves instead of dropping events when full.
- **`SamplingClock::tick`** (`flui-interaction`) returns a window for `Manual` clocks too.
- **`PointerPanZoomEvent::Update`** (`flui-interaction`): `scale` and `rotation` are documented as
  per-tick deltas (as delivered); a non-finite or non-positive scale and a non-finite rotation
  become the identity.

### Fixed

- Velocity trackers (`flui-interaction`) publish finite estimates bounded by
  `DEFAULT_MAX_FLING_VELOCITY` for any finite samples, and the least-squares tracker falls back to a
  linear fit when samples carry only two distinct timestamps instead of reporting zero velocity.
- `Velocity::clamp_magnitude` no longer panics on inverted or NaN bounds.
- `RawInputHandler` no longer panics when its callback replaces or clears itself.
- `InputPredictor` output stays finite and bounded: the acceleration term comes from the
  least-squares fit and is capped, and an out-of-range smoothing factor is clamped.
- The resampler's queue-overflow diagnostic runs after committing the event and releasing its
  lock, so tracing subscribers can inspect or enqueue through the same resampler.
