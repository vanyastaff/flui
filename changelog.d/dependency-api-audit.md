### Changed

- `NetworkLoader::new()` now returns `reqwest::Result<NetworkLoader>` so TLS and resolver initialization failures can be handled. `NetworkLoader` no longer implements `Default`; callers use `NetworkLoader::new()?` or supply a configured client through `with_client`.
- Asset registries reuse an HTTP connection pool across bridged network image loads. `AssetRegistryBuilder::with_network_client` accepts a fresh client builder so hosts can configure HTTP deadlines, headers, proxies and redirects without importing connections from another runtime.
- The AI streaming example reuses its provider HTTP client and reads response chunks without enabling reqwest's optional Stream integration. JSON support is enabled only by the example's development dependency.
- Network images isolate cached responses and pending loads by asset registry as well as URL. `ImageCacheKey::Network` now contains an opaque `NetworkImageKey` and is available with `network-images`; obtain keys through `ImageProvider::cache_key`.
