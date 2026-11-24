### Fixed

- Detaching a `WorkerReloadHook` whose artifact watcher had died on a panic (its `tracing`
  events run the application's subscriber) logs that the watcher had stopped and destroys the
  panic's payload under containment; a payload whose destructor panicked used to make the detach
  panic in turn.
