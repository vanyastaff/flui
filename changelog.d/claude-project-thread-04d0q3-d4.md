### Fixed

- **`flui-view`**: ten or more nested `LayoutBuilder`s (or fewer around a lazy list) no longer
  panic with `BUG: layout<->build fixpoint failed to converge` in debug builds, or paint their deep
  levels unbuilt in release builds. Nesting costs the frame one pass per level, on the first frame,
  after a rebuild above the builders and after a resize, and those passes no longer count against
  the frame's bound; only passes in which a builder rebuilds with nothing above it rebuilt do
  ([ADR-0308](/docs/adr/ADR-0308-layout-build-fixpoint-charges-only-passes-without-progress.md)).

### Changed

- **`flui-view`**: a `LayoutBuilder` whose output keeps changing its own constraints now logs one
  error and paints the last laid-out tree in every build profile, instead of panicking in debug
  builds.
