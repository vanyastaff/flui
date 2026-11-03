### Added

- `flui_native::http::UreqClient`: the built-in HTTP client of Linux and the BSDs, `ureq` over rustls with the platform's verifier, each request on a thread of its own (`flui-native`'s `http` feature, which `flui-app`'s turns on).

### Changed

- Linux sends HTTP through `ureq` instead of `reqwest`, with the system's trust store and the proxy the environment names; an embedder's executors there send too, where they answered `HttpError::NotYetOnThisHost`. With every host on a stack of its own, `flui-app` no longer depends on `reqwest`, and its `http` feature now only turns on `flui-native`'s.
- An exchange held on the IO lane, on every host, ends with `HttpError::Cancelled` when the lane shuts down under it.
