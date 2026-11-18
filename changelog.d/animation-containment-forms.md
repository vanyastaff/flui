### Changed

- `AnimationRunFuture::when_complete_or_cancel` takes an `FnOnce`, so a continuation may move its
  captures out (a `oneshot::Sender`, a value for the next step). Delivery keeps the first panic
  with `FirstPanic` and wakes each waiter by value.
- Built-in curve values and `PiecewiseLinear` stops are shared through `Rc` instead of `Arc`;
  curves were already owner-thread values (`SharedCurve` is neither `Send` nor `Sync`).

### Removed

- `impl Curve for Arc<C>`: pass the curve itself, or an `Rc<dyn Curve>`.
- The serde implementations of `AnimationError`, `AnimationInput`, `RangeDefect`,
  `SimulationError` and `SimulationParameter`, and their `NaN`/`Infinity` wire tokens (ADR-0381).
