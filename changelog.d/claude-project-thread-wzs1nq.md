### Added

- `flui::semantics`: the vocabulary `SemanticsConfiguration` takes, as Flutter's semantics
  library exports it: tags, sort keys, hint overrides, relations, collections, states, text
  content, custom actions and their handlers, announcements. Each was reachable only by an internal
  crate path. `flui::painting` adds `SemanticsRegionId` beside the region types that name it.
