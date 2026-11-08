### Changed

- **`flui-platform` / `flui-system`**: `LaunchError` and `DialogError` embed the shared `Unsupported` value, expose it through `unsupported()`, and preserve the `launcher` or `file-dialog` capability name in their messages and error sources.

### Removed

- **`flui-platform` / `flui-system`**: `LaunchError::Unavailable`, `DialogError::Unavailable`, and their bare `UnsupportedReason` payloads. Construct either error from `Unsupported` instead.
- **`flui-platform`**: `HttpHost` and `HttpError::NotYetOnThisHost`. Every supported host has an HTTP provider; a build without the `http` feature leaves the capability unregistered instead of constructing an exchange error.
