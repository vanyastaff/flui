### Added

- `flui_native::http::WinHttpClient` (Windows): an `HttpClient` over the system's WinHTTP, with the
  system's proxy settings and trust store. Each request runs on a thread of its own and reads its
  body a chunk at a time, only as fast as it is read; dropping a request or its body aborts it, and
  the request's deadline aborts it with `HttpError::Timeout`.

### Changed

- On Windows, `HttpCapability` sends through WinHTTP instead of `reqwest`, each exchange holding a
  slot of the run's IO lane until its body ends or is dropped, so it also sends on executors an
  embedder passes to `AppConfig::with_executors`, which answered `HttpError::NotYetOnThisHost`.
  `flui-app` no longer links `reqwest` or `rustls` on Windows.
