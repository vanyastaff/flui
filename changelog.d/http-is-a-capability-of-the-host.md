### Added

- **`flui-platform`**: HTTP is a capability of the host (ADR-0280). `flui_platform::http` has the
  `HttpClient` contract, whose futures need no async runtime of the caller's, the `HttpCapability`
  marker with its `Send` handle `Http`, `HttpRequest`, `HttpResponse` with a chunked
  `ResponseBody`, and a typed `HttpError`. Behind `testing`, `ScriptedHttp` answers scripted
  responses per URL, so no test reaches the network. `HttpClient::authority` returns an
  `HttpAuthority` naming the state a client's responses come from, so caches keep two clients'
  responses apart.
- **`flui-app`**: the built-in HTTP provider, behind the default `http` feature, sends `reqwest`
  requests from the run's IO lane. Android, the web and an embedder's executors answer
  `HttpError::NotYetOnThisHost`.
- **`flui-testing`**: `HeadlessHost::http` is the host's scripted HTTP client, registered among its
  built-in capabilities.
- **`flui-widgets`**: `ImageServices`, the lanes and HTTP client an `ImageProvider`'s own loader
  runs on.
- **`flui-runtime`**: `ExecutionServices::owns_default_pools` is available outside `testing`: it
  tells FLUI's own pools from lanes an embedder injects.

### Changed

- **`flui-assets`**: `AssetRegistry::load_image` and `load_network_image` take `LoadServices`
  (lanes and an HTTP client) instead of lanes alone, and a network image is fetched with the
  caller's client. `ImageSource::Network` holds a `NetworkSource`, the URL with the authority of
  the client that fetches it (`ImageSource::network`), so one registry keeps two clients' images
  of one URL apart.
- **`flui-widgets`**: `ImageProvider::resolve_async` takes `&ImageServices` instead of
  `Option<&Background>`. `Image` acquires `HttpCapability` beside `BackgroundCapability`, so
  `Image::network` loads on the headless host. `NetworkImage` and `Image::network` need only the
  `asset-images` feature. `ImageProvider::cached` takes `&ImageServices`, and
  `NetworkImage::resolve` always answers `RequiresAsyncResolve`: which image is cached depends on
  the client.

### Removed

- **`flui-assets`**: the `network` feature, `NetworkLoader`, `AssetRegistryBuilder::with_network_client`,
  the `reqwest` dependency and `AssetError::ReactorUnavailable` and `AssetError::NetworkError`, replaced
  by `AssetError::HttpUnavailable`, `AssetError::Http` and `AssetError::HttpStatus`.
- **`flui-widgets`**: the `network-images` feature.
