### Added
- `SceneOwner::completed`: the raster side reports the version it holds; the owner's next `publication` takes it as its base and publishes the union of the versions applied since as its damage, through a pending log of sixteen entries that folds its two oldest instead of losing a change.
- `ReleasedContents`: what `completed` and `renew` let go of, to be dropped after the owner's borrow ends.

### Changed
- `SceneOwner::renew` returns the old lineage's version and log as `ReleasedContents`, and `publication` is no longer always full.
