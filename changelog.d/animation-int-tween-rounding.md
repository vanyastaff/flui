### Changed

- **One `IntTween` with an explicit `Rounding`** (`flui-animation`): integer tweens choose
  `Rounding::Nearest` (the default), `Floor`, `Ceil` or `TowardZero` with
  `IntTween::with_rounding`. A serialized `IntTween` without a `rounding` field decodes as
  `Nearest`.

### Fixed

- **`IntTween` rounds halves toward positive infinity** (`flui-animation`): an interpolated −2.5
  is now −2, not −3, as CSS Values 4 §5.2.1 interpolates integers.

### Removed

- **`StepTween`** (`flui-animation`): use `IntTween::new(begin, end).with_rounding(Rounding::Floor)`;
  a serialized `StepTween` has no automatic migration and decodes as nearest rounding.
