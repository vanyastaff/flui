### Added

- `flui_native::http::UrlConnectionClient` (Android): an `HttpClient` over the system's
  `HttpURLConnection`. Each request runs on a thread of its own and reads its body a chunk at a
  time, only as fast as it is read; dropping a request or its body disconnects it, and the
  request's deadline disconnects it with `HttpError::Timeout`.
- `HttpError::Host`: the host's bridge to its network stack failed outside the exchange.

### Changed

- On Android, `HttpCapability` sends through `HttpURLConnection` instead of answering
  `HttpError::NotYetOnThisHost`. Each exchange holds a slot of the run's IO lane until its body
  ends or is dropped, as on the web.

### Removed

- `HttpHost::Android`: Android has an HTTP client.
