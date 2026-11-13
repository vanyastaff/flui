### Added

- `ReleasedContents::reclaim` runs the destructors of what a scene owner released and keeps,
  emptied, the version records, table nodes and contents nobody else held; `SceneOwner::restock`
  takes them back, so the next applies and composited-layer updates write into them instead of
  allocating.

### Changed

- The composited-layer update is `SceneWriter::with_effects(&content, replaced)`, which writes
  into a content the owner reclaimed; `BoundaryContent::with_effects` is removed.
- A scene owner keeps the version an apply replaced until no other holder reads it, then hands it
  back for reclaiming; the UI runtime and `SceneHost` restock what each frame releases.
