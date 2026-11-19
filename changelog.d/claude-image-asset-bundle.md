### Changed

- `Image::asset(key)` and `AssetImage::new(key)` read the application's asset `key` (`assets/logo.png`, its path from the project root as `[assets] directories` declares it) through the host's asset bundle (`AssetBundleCapability`, ADR-0384 §6) instead of opening a path relative to the process's current directory, so the same key shows the same image from an APK, an iOS or macOS bundle, a Web page and a desktop package. `AssetImage::path` is now `AssetImage::key`.
- `flui_assets::ImageSource::asset(key, &bundle)` takes an `AssetKey` and the `AssetBundle` that reads it, and `ImageSource::path` is now `ImageSource::asset_key`; the image cache keeps two bundles' images apart under one key.
- `ImageLoadErrorKind`'s `NotFound`, `NotAFile` and `Io` messages speak of the asset, not a file.

### Added

- `ImageServices::with_asset_bundle` and `asset_bundle`, which `Image` fills from `AssetBundleCapability`; `ImageProviderError::AssetBundleUnavailable` for a host without a bundle and `ImageProviderError::InvalidAssetKey` for a key no bundle can hold.
