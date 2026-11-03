### Added

- `flui_native::http::FetchClient` (wasm32): an `HttpClient` over the page's `fetch`. Dropping a
  request or its body aborts the fetch, and the request's deadline aborts it with
  `HttpError::Timeout`.

### Changed

- On the web, `HttpCapability` sends through the page's `fetch` instead of answering
  `HttpError::NotYetOnThisHost`. Each exchange holds a slot of the run's IO lane until its body
  ends or is dropped, so a full or stopping lane refuses it with `HttpError::Refused`.

### Removed

- `HttpHost::Web`: the web has an HTTP client.
