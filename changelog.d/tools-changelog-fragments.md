### Added

- **Changelog fragments**: unreleased entries arrive as fragments under `changelog.d/`
  (one file per branch, a Keep a Changelog `###` section and bullets under it), so pull requests
  no longer edit the same lines of `CHANGELOG.md`. CI's `checks` job validates every fragment and
  the `## [Unreleased]` region; at release the fragments are merged into that region by hand,
  above the bullets already there, and deleted. The format is in
  [`changelog.d/README.md`](/changelog.d/README.md).
