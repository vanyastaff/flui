### Added

- The facade's `images` feature: `Image::asset` and `Image::network`, and `flui::assets`. Each UI
  runtime keeps one image cache, `flui::assets::ImageCacheCapability`, which every image of that
  runtime loads through and the runtime trims under memory pressure (ADR-0387).
- `ProviderContext::runtime_cache` and `RuntimeLocals::get_or_default_reclaimed`: per-runtime
  provider state that the runtime trims under memory pressure.
- `ImageProviderError::ImageCacheUnavailable`, answered by an asset or network image on a host
  without an image cache, and `ImageServices::with_image_cache` / `image_cache`.

### Changed

- `Image::asset(path)`, `Image::network(url)`, `AssetImage::new(path)` and
  `NetworkImage::new(url)` take no cache: they load through their UI runtime's.
  `ImageCacheKey` of an asset or network image is its path or URL alone.
  An application no longer registers an image cache with `AppConfig::memory_reclaim`.
