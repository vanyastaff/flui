### Added

- Add the off-by-default facade `assets` feature, exposing `flui::assets` and
  enabling asset/network image widgets without changing the prelude.
- Let `AssetRegistryBuilder::new().build()` construct an image registry with a
  platform-appropriate decoded-byte budget and decode concurrency.

### Removed

- Remove the generic asset trait, type-erased caches, typed handles and keys,
  font/image descriptors, byte loaders, type-state capacity markers and their
  Moka/Tokio-filesystem implementation. `AssetRegistry` now has only its
  decoded-image cache and shared file/network loads.
