### Removed

- `flui_platform::http::HttpError::NotYetOnThisHost` and the `HttpHost` type it carried.
  No host ever answered that variant, and a host whose HTTP provider has not landed now
  reports the capability as absent via `Unsupported` (the unified absence value) instead
  of an `HttpError`. ADR-0280's "More information" is rewritten to match.
