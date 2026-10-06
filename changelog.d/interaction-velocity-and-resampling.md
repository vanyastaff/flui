### Added

- **`VelocityTracker::estimate_at` / `velocity_at`** (`flui-interaction`): query a velocity as of a
  time on the samples' own clock, so the 40 ms "pointer stopped" gate is deterministic under
  virtual clocks and replay.
- **`PointerEventResampler::add_event_at`** (`flui-interaction`): queue an event at an explicit
  time on the sampling clock.

### Changed

- **`GestureSettings::clamp_fling_velocity`** (`flui-interaction`) clamps the magnitude to the
  maximum fling velocity and keeps the sign; it no longer raises slow velocities to the minimum
  (which flipped negative velocities) and never panics. `max_fling_velocity()` is never below
  `min_fling_velocity()`. Negative or non-finite settings values are sanitized: `new` uses the
  touch default, `with_*` builders keep the previous value.
- **`PointerEventResampler`** (`flui-interaction`) is an owner-thread handle (`Rc<RefCell<_>>`,
  no longer `Send`/`Sync`); it places events by their own time, interpolates at
  `(sample - last) / (next - last)`, and coalesces moves instead of dropping events when full.
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
