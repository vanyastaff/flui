### Changed

- **`flui-app`**: `CloseRequestHandler::new` takes a closure without `Send` or `Sync`: a close
  handler runs on the thread that opened the window and may capture `Rc`, `Cell` and `RefCell`
  state. `AppConfig` is not `Send`: it is built and read on the thread that runs the
  application.
- **`flui-app`**: `run_direct` takes a render closure without `Send`, and a resize that arrives
  while the closure builds its scene is applied.
- **`flui-app`**: `AppConfig::memory_reclaim` takes a receiver without `Sync`.

### Removed

- **`flui-app`**: `CloseRequestError::WrongThread`. `request_presentation_close` from a thread
  that runs no FLUI loop answers `NoHostedRuntime`, as before; no thread could receive
  `WrongThread`.
