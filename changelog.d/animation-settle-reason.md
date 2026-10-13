### Added

- **Completion receipts say why a run settled** (`flui-animation`, `diagnostics` feature): a
  `complete` transition receipt carries `settle_reason`, one of `clock`, `reduced_motion`,
  `exhausted_clock` or `stopped`, so a completion forced by the reduced-motion policy can be told
  apart from a natural one.
