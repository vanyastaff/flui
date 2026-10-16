### Fixed

- Reuse unchanged `check-changed` verification results across Heli runs by
  keying managed scheduling independently of its per-run ID, while preserving
  other environment and configuration inputs.
