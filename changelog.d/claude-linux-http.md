### Added

- `flui_native::http::UreqClient`: the built-in HTTP client of Linux and the BSDs, `ureq` over rustls with the platform's verifier, each request on a thread of its own (`flui-native`'s `http` feature, which `flui-app`'s turns on).

### Changed

- Linux sends HTTP through `ureq` instead of `reqwest`, with the system's trust store and the proxy the environment names; an embedder's executors there send too, where they answered `HttpError::NotYetOnThisHost`. `reqwest` remains on macOS and iOS.
- An exchange held on the IO lane (the web, Android, Windows, Linux) ends with `HttpError::Cancelled` when the lane shuts down under it.
