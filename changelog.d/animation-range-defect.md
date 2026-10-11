### Changed

- `ValueRange::new()` returns `RangeDefect` instead of `AnimationError`, distinguishing non-finite endpoints, inverted bounds and span overflow without parsing text. The admitted ranges and validation order are unchanged.
- Animation admission errors carry typed input kinds, endpoint values, component indices and original simulation causes instead of `InvalidBounds(String)`, `InvalidSpring(String)` and `NonFiniteTarget(String)`. Match the new structured variants; optional serde error shapes change accordingly.

### Fixed

- Repeating over finite endpoints whose difference overflows `f64` refuses before replacing the installed run, preserving its displayed value and completion future.
- Optional serde round-trips non-finite error diagnostics using `NaN`, `Infinity` and `-Infinity` strings in human-readable formats, including nested simulation causes. Finite numbers and native binary floats retain their encoding; NaN payload bits are canonicalized.
