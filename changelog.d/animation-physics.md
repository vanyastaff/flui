### Changed

- `flui-animation`: `SpringDescription` stores a validated natural frequency and
  damping ratio with private fields. `SpringDescription::new`,
  `with_duration_and_bounce` and `with_response_and_damping` return
  `Result<_, SimulationError>`, and the perceptual constructors take a
  `Duration`; an undamped or non-finite spring is refused in debug and release.
  `SpringSimulation::try_new` takes a `Tolerance` and returns `Result`;
  `SpringSimulation::new` and `SpringDescription::with_damping_ratio` remain for
  constants and panic on non-finite or non-positive input; where `try_new`
  would refuse finite inputs as unrepresentable, `SpringSimulation::new`
  returns a spring already at rest at `end`.
- `flui-animation`: every spring `try_new` builds publishes finite samples at
  every time and rests at a finite time; a position past the `f64` range
  saturates at `±f64::MAX`, and a spring whose rest or oscillation phase is not
  representable is refused with `SimulationError::Overflow`.
- `flui-animation`: springs are evaluated in one closed form for every damping
  ratio, finite for every time (including very heavy damping and `t = ∞`), and
  continuous through critical damping.
- `flui-animation`: every simulation rests at a time computed when it is built.
  From then on `x` is exactly the resting position, `dx` is `0.0` and `is_done`
  stays `true`, independent of frame timing. A time before the start (negative
  or NaN) yields the initial state.
- `flui-animation`: `FrictionSimulation::new` and
  `BoundedFrictionSimulation::new` take a `Tolerance` (and `SimulationBounds`)
  and return `Result`; friction rests by its remaining glide, and
  `time_at_x` answers `+inf` for a position the motion never reaches.
- `flui-animation`: `Tolerance` is built with `Tolerance::new(distance, velocity)`
  or `Tolerance::for_device_pixel_ratio`; an infinite velocity limit derives one
  from the motion's time scale. `Simulation::tolerance` has a default body.
- `flui-animation`: `AnimatedValue::new`, `animate_to` and `set_value` refuse
  non-finite components and leave the value unchanged; `advance` takes a
  `Duration`.
- `flui-widgets`: `ScrollMetrics` carries `device_pixel_ratio`
  (`with_device_pixel_ratio`); `Scrollable` and `RefreshIndicator` read it from
  the presentation at release, so a fling rests within half a device pixel
  (an 8000 px/s fling at drag 0.135 ends at 4.49 s instead of 7.94 s).
- `flui-widgets`: `BouncingScrollPhysics` flings into an edge overscroll and
  spring back instead of stopping dead; unordered extents start no ballistic
  run instead of panicking.

### Added

- `flui-animation`: `SimulationError`, `SimulationParameter`,
  `SimulationBounds` and `BouncingScrollSimulation`.

### Removed

- `flui-animation`: the `smoothing` module (`exp_decay`, `exp_decay_half_life`,
  `Smoothed`, `SmoothDamp`) and its example; `ScrollSpringSimulation`,
  `GravitySimulation`, `ClampedSimulation`, `FrictionSimulation::through` and
  `with_tolerance`, `SpringSimulation::with_snap_to_end` and `end_position`,
  `SpringDescription::{damping_ratio, bounce, smooth, snappy, bouncy}` and its
  public fields, and `Tolerance`'s public fields.
