### Changed

- **`AppHandle::request_quit` can be refused.** The quit is asked of every presentation on the
  owner's next turn as a close for `CloseReason::Program`: its close guard first, then its
  close-request handler. When one refuses, the quit is deferred until each recorded close is
  carried out, and cancelled when one is resolved with `PendingClose::stay_open` or a refusal
  records nothing (a handler that panicked). New work is fenced only once the quit goes ahead,
  not when it is asked for.
- **`CloseResponse::KeepOpen` records the close.** A handler's `KeepOpen` records a
  `PendingClose` for the request's reason on the window's close guard, which the window's widgets
  resolve with `discard_and_close` or `stay_open`.

### Fixed

- `CloseRequest::reason` reads the reason the close was asked for; it always read
  `CloseReason::User`.
