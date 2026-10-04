### Changed

- Image and network asset types now require their respective `images` and `network` features; file-backed assets use the real byte loader ([ADR-0107](/docs/adr/ADR-0107-asset-byte-sources-and-decoding.md)).
- Asset registry APIs accept decoded data without `Clone`; cloned handles share ownership. Handle identity comparison is now the inherent `AssetHandle::ptr_eq` operation.

### Fixed

- Handle identity comparison distinguishes retained and reloaded allocations with equal asset keys.

### Removed

- Removed the unsupported `AssetLoader` trait, generic `FileLoader`, and unused `MemoryLoader`; custom source selection and decoding belong in `Asset::load`.
