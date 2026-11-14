### Changed

- `flui run --agent` in Firefox needs Firefox 140 or later, the first that reports when a navigation's document commits. An older Firefox, ESR 128 among them, is refused when the session starts, with its version and the one needed; before, it ran with a race on every reload.

### Fixed

- `flui run --agent` in Firefox ends a page's agent connections when the reloaded document commits, not when the reload starts. A client that connects while the reload runs has its connection ended at the commit and connects again to the reloaded page, instead of being left on a connection the old page took with it.
