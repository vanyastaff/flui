### Fixed

- Dropping a UI runtime whose teardown raised a failure no longer aborts the process when a
  queued cross-thread signal write still holds a capture whose destructor panics: the runtime
  destroys its queued commands and frame-failure handler itself, or retains them once a failure
  is held, before it raises.
