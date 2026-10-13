### Changed

- **`SimulationError::NonFinite { parameter }`** (`flui-animation`): a NaN or infinite simulation,
  spring or tolerance input is reported as `NonFinite` instead of being folded into `OutOfRange`,
  which now means a finite value outside the parameter's admitted range. Its message names that
  range (`mass = 0 is outside (0, ∞)`), as `CurveError` does.
- **`SimulationParameter::UnitDistance`** (`flui-animation`): a spring curve's distance tolerance,
  which must be below 1 on its unit motion, is reported as `UnitDistance` rather than `Distance`,
  whose range is `(0, ∞)`.
- **`LinearEasingError` is `PiecewiseLinearError`** (`flui-animation`), named after the
  `PiecewiseLinear` curve it refuses.
