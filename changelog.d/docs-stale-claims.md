### Changed

- **`cargo xtask` is the repository's task runner**: it replaces the justfile, `scripts/` and the
  pre-push hook, and CI calls the same commands a contributor runs (`cargo xtask --help`; the
  job table is in [`docs/testing.md`](/docs/testing.md)). Crate tiers and layers are
  `[package.metadata.flui]` keys in each manifest, checked by `cargo xtask workspace`. Only the
  macOS and iOS device drivers stay scripts, in `tools/device-checks`.
