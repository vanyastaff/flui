### Added
- `SceneOwner::completed`: the raster side reports the version it holds; the owner's next `publication` takes it as its base and publishes the union of the versions applied since as its damage, through a pending log of sixteen entries that folds its two oldest instead of losing a change.
- `ReleasedContents`: what `apply`, `completed` and `renew` let go of, to be dropped after the owner's borrow ends.
- `Applied`: what `SceneOwner::apply` returns, the new version's id and its `ReleasedContents`.

### Changed
- `SceneOwner::apply` returns `Applied` instead of the bare id, and `SceneOwner::renew` returns the old lineage's version and log as `ReleasedContents`, and `publication` is no longer always full.
