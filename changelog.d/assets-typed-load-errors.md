### Changed

- `flui_assets::ImageCache` takes one request shape: `load(&ImageSource, &LoadServices)` and the
  probe `get(&ImageSource)` replace `load_image`, `load_network_image` and `cached_image`. A
  source is built with `ImageSource::asset(key, &bundle)` or `ImageSource::network(url, &http)`, which
  carries the reader that fetches it; `LoadServices` carries only lanes, set with `lanes(…)`
  (ADR-0386).
- A failed load answers `ImageLoadError` (its source and an `ImageLoadErrorKind`: `NotFound`,
  `NotAFile`, `TooLarge`, `Io`, `Decode`, `Http`, `HttpStatus`, `LanesRefused`, `Panicked`,
  `Abandoned`) in place of `AssetError`. `flui-widgets` maps a missing file to
  `ImageProviderError::SourceNotFound` and an undecodable one to `DecodeFailed`.

### Added

- `ImageProviderError::HttpUnavailable`: a `NetworkImage` on a host with no HTTP client.

### Removed

- `flui_assets::AssetError`, `flui_assets::Result`, `NetworkSource`, `LoadServices::with_lanes`
  and `LoadServices::with_http`.

### Fixed

- An HTTP client or a file read that panics fails its image load with `Panicked` instead of
  unwinding through the lane that polled it; an executor that caught the unwind no longer makes
  the waiters call the panicking client again.
