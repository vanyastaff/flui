### Changed

- **`flui-animation`**: `Tolerance::try_new(distance, speed)` takes a `SpeedLimit`
  (`Derived` or `AtMost(f64)`) instead of a velocity float where `f64::INFINITY` meant
  "derive it". An infinite or non-positive `AtMost` speed is refused, naming the new
  `SimulationParameter::SpeedLimit`
  ([ADR-0200](/docs/adr/ADR-0200-dynamics-derived-rest-velocity.md)).
