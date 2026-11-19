### Changed

- **`flui-app`**: `CloseRequestHandler::new` takes a closure without `Send` or `Sync`: a close
  handler runs on the thread that opened the window and may capture `Rc`, `Cell` and `RefCell`
  state.
- **`flui-app`**: `AppConfig::memory_reclaim` takes a receiver without `Send` or `Sync`: a
  receiver is reclaimed on the thread that runs the application.
- **`flui-app`**: `AppConfig`, `CloseRequestHandler` and `Application` are neither `Send` nor
  `Sync` on any target: they are built and read on the thread that runs the application. Code
  that moved one to another thread no longer compiles.

### Removed

- **`flui-app`**: `CloseRequestError::WrongThread`. `request_presentation_close` from a thread
  that runs no FLUI loop answers `NoHostedRuntime`, as before; no thread could receive
  `WrongThread`.
