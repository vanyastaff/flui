### Changed

- `flui run --agent` in Firefox needs Firefox 140 or later, the first that reports when a navigation's document commits; an older Firefox is refused at the start, naming the version.

### Fixed

- `flui run --agent` in Firefox ends a page's agent connections when the new document commits, not when its navigation starts, so a client that connects during a reload reaches the reloaded page instead of the one it leaves.
