### Changed

- `flui run --agent` in Firefox needs Firefox 140 or later, the first that reports when a navigation's document commits. An older Firefox, ESR 128 among them, is refused when the session starts as an unsupported target (exit 2), with its version and the one needed; before, it ran with a race on every reload.

### Fixed

- `flui run --agent` in Firefox ends a page's agent connections when the reloaded document commits, not when the reload starts. A connection opened while the reload runs is no longer left on the old page, which takes it with it: it is ended at the commit, and `flui mcp` connects again to the reloaded page.
