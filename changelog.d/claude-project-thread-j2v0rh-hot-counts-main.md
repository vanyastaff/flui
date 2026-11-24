### Fixed

- `WorkerPollOutcome::Reloaded`'s `reload_count` is the number of worker images the driver has
  loaded, the first included, also when a reload was refused in between: the load after a refusal
  counted from one again.
