### Added

- **`flui-assets`**: `AssetRegistry::cached_image`, `ImageSource` and `ImageLoad`; the registry
  keeps the decoded images of bridged loads within a byte budget
  (`AssetRegistryBuilder::with_decoded_image_budget`) and bounds concurrent decodes
  (`with_decode_threads`)
  ([ADR-0250](/docs/adr/ADR-0250-asset-registry-keeps-decoded-images.md)).
- **`flui-widgets`**: `ImageProvider::cached`, the non-blocking probe `Image` makes before it
  starts a load, and `ImageCacheKey::custom` for a custom provider's typed key.

### Changed

- **`flui-assets`**: `load_image_bridged` and `load_network_image_bridged` return `ImageLoad`,
  share in-flight loads and serve decoded images from the registry's cache; a second load of a
  decoded source is a hit, not a new read or request.
- **`flui-assets`**: `NetworkLoader::new` gives up after `CONNECT_TIMEOUT` and `READ_TIMEOUT`,
  and `load_url` refuses a body over `MAX_BODY_BYTES` (256 MiB).
- **`flui-widgets`**: `ImageCacheKey` is opaque. Asset and network images are keyed by their
  registry as well as their source, so equal paths in two registries are two images.

### Removed

- **`flui-widgets`**: the process-wide decoded-image cache, `ImageCacheKey::Asset`,
  `ImageCacheKey::Network` and `NetworkImageKey`.

### Fixed

- **`flui-assets`**: dropping the last waiter of a bridged image load aborts it: the HTTP request
  stops, the file read stops at its next chunk and the decode never runs, so abandoned loads no
  longer delay visible ones. At most four loads per decode thread read or wait to decode at
  once, and loads are served in the order they were requested.
- **`flui-assets`**: bridged decodes run on the loading runtime's blocking pool, never on a host
  runtime's async workers.
- **`flui-assets`**, **`flui-engine`**: memory pressure frees decoded images (in a registered
  asset registry) and GPU image textures, including while frames are stopped: moderate pressure
  keeps what the last frame drew, critical pressure frees all of it, including the image atlas's
  GPU texture.
