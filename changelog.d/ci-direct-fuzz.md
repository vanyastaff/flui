### Removed

- `cargo xtask fuzz`. The nightly fuzz workflow builds each target with
  `cargo +nightly --config .cargo/fuzz.toml run`, the flags a fuzz build adds live in
  `.cargo/fuzz.toml`, and each target's package, seeds, longest input and limits are a row of
  `fuzz.yml`'s matrix. docs/testing.md ("Fuzzing") shows a run and the replay of a finding.

### Fixed

- The nightly fuzz workflow carries each target's findings to the next night and reads them back
  as inputs, so a red night stays red until its fix lands instead of turning green when a run
  does not reach the same path again.
