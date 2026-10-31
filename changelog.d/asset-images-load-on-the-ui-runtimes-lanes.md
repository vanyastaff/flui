### Changed

- **`flui-assets`**: `AssetRegistry::load_image` and `load_network_image` (formerly
  `load_image_bridged` and `load_network_image_bridged`) take the background lanes to load on,
  `Option<Arc<dyn flui_platform::executor::Executor>>`, and the registry starts no runtime or
  thread of its own: the read runs on the IO lane and the decode on the compute lane. Without
  lanes, or with lanes too full to take it, the load runs where its `ImageLoad` is polled. A load
  stays shared by every request for its source: one request leaving no longer risks it, and a load
  dropped unfinished by its request or its lanes is taken over by the next request still waiting.
- **`flui-widgets`**: `ImageProvider::resolve_async` takes the UI runtime's
  `Option<&Background>`; `Image::asset` and `Image::network` load on the UI runtime's lanes.

### Added

- **`flui-view`**: `Background::executor`, the lanes themselves, for a crate below the view layer
  that runs its own work on them.
- **`flui-assets`**: `AssetError::ReactorUnavailable`, a network image loaded on an IO lane without
  a Tokio reactor; `AssetError::LanesRefused`, lanes shutting down.

### Removed

- **`flui-assets`**: `AssetRegistryBuilder::with_runtime_handle` and the registry's own Tokio
  runtime.
