### Fixed

- Deliver secondary presentations' queued pointer motion and deferred gesture decisions at frame cadence, and refresh stationary hover against each window's own committed tree.
- Discard queued pointer motion across suspension while preserving ordinary window-blur hovering and completed input awaiting its first presentation.
- Complete suspended pointer-state cleanup even when its tracing subscriber panics, preserving an earlier terminal callback failure.
