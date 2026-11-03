### Added

- `flui_native::http::UrlSessionClient` (macOS, iOS): an `HttpClient` over an ephemeral
  `NSURLSession`. Each request runs on a thread of its own; its task is suspended while 256 KiB of
  its body wait unread, and dropping a request or its body, or its deadline, cancels the task.

### Changed

- On macOS and iOS, `HttpCapability` sends through `NSURLSession` instead of `reqwest`, so the
  system's proxy settings and trust store apply, and an embedder's executors need no Tokio there.
  Each exchange holds a slot of the run's IO lane until its body ends or is dropped.
