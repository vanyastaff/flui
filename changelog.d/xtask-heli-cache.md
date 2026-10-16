### Fixed

- Reuse unchanged `check-changed` verification results across Heli runs by
  keying managed scheduling independently of its per-run ID, while preserving
  other environment and configuration inputs.
- Run the complete xtask test suite and strict Clippy on Linux, Windows and
  macOS for changes to the host tool or its build inputs.
