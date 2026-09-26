### Added

- **`cargo xtask changelog`**: unreleased entries arrive as fragments under `changelog.d/`
  (one file per branch, a Keep a Changelog `###` section and bullets under it), so pull requests
  no longer edit the same lines of `CHANGELOG.md`. `--check` validates every fragment and the
  `## [Unreleased]` region and runs in `cargo xtask checks`; at release the command merges the
  fragments into that region, newest bullets first, and removes them. The format is in
  [`changelog.d/README.md`](/changelog.d/README.md).
