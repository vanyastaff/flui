### Changed

- `RepeatSpec` and `MotionSpec` take bare builder names (ADR-0325): `RepeatSpec::range`,
  `period` and `iterations`, and `MotionSpec::duration` and `curve`, replace their `with_` forms.
