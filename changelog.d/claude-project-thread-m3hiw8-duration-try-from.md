### Changed

- `Milliseconds` and `Seconds` convert to `std::time::Duration` with `TryFrom`, whose error is
  `std::time::TryFromFloatSecsError`. The `From` conversions are gone: write
  `Duration::try_from(ms)` where the code had `ms.into()`.

### Fixed

- Converting a negative, NaN or too large `Milliseconds` or `Seconds` to `std::time::Duration`
  returns an error. It panicked inside `Duration::from_secs_f64`.
