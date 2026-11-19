### Added

- `flui build` packages the files `[assets] directories` declares in `flui.toml`: every regular file under each directory, but a hidden one, is copied to `flui_assets/<key>` beside a desktop executable, in a macOS `.app`'s `Contents/Resources`, at an iOS bundle's root, in an APK's `assets/` (through `aapt2 -A`, or a Gradle project's `flui.assetsDir` property) and beside a Web page; a key the asset bundle would refuse, two files whose keys differ only in case and a symbolic link fail the build (ADR-0384 §3).
- A desktop `flui run` starts the application with `FLUI_ASSET_ROOT` set to the project root, so an edited asset is read on its next load without a copy (ADR-0384 §4).
