### Fixed

- `flui run --agent` in Firefox ends a page's agent connections when the new document commits, not when its navigation starts, so a client that connects during a reload reaches the reloaded page instead of the one it leaves.
