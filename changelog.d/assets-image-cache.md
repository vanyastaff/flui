### Changed

- `flui_assets::AssetRegistry` is `ImageCache`, configured with `ImageCacheConfig`
  (`budget`, `decode_threads`) in place of `AssetRegistryBuilder`; `ImageCache::default()` is the
  default budget and decode threads. `flui-widgets`' `AssetImage::new`, `NetworkImage::new` and
  `Image::asset`/`Image::network` take an `Arc<ImageCache>`.

### Removed

- The typed asset framework of `flui-assets`: the `Asset` trait, `AssetCache`, `AssetHandle`,
  `WeakAssetHandle` and their `…Core`/`…Ext` traits, `AssetKey`, `AssetMetadata`, `LoadState`,
  `CacheStats`, `CacheCapacity`, `CacheExpiration`, `AssetCacheConfig`, `FontAsset`, `FontData`, `ImageAsset`,
  `BytesFileLoader`, `AssetRegistryBuilder`, the `images` and `full` features and the
  `flui_assets::Image` re-export. Nothing in FLUI loaded through it; fonts are registered with
  `flui_app::register_font` (ADR-0385).
