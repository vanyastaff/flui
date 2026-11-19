### Added

- `flui_platform::assets`: `AssetKey`, the allow-listed project-relative name of an application's asset, and `AssetBundleCapability`, whose `AssetBundle` handle reads an asset by key (`load`, `load_string`) through the host's `AssetSource`, with `DirectoryAssets` for a directory of files and `flui_platform::testing::MemoryAssets` for a test's own assets (ADR-0384).
- Every host provides the application's asset bundle: `flui_native::assets::package_assets` reads `flui_assets/` beside the executable or under the main bundle's resources (or `FLUI_ASSET_ROOT` in a desktop development run), `ApkAssets` reads an APK's assets through `AAssetManager`, and `PageAssets` fetches them beside a Web page.
- `HeadlessWindow::with_assets` names the assets a headless test's widgets read; without it they read none.
