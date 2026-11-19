### Added

- `flui_platform::assets`: `AssetKey`, the allow-listed project-relative name of an application's asset, and `AssetBundleCapability`, whose `AssetBundle` handle reads an asset by key (`load`, `load_string`) through the host's `AssetSource`, with `DirectoryAssets` for a directory of files and `flui_platform::testing::MemoryAssets` for a test's own assets (ADR-0384).
