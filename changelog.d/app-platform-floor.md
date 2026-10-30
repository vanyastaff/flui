### Added

- `[platforms.<environment>] min` in `flui.toml` raises FLUI's floor for `windows`, `macos`, `ios` or `android` (ADR-0244 §10). A value below FLUI's floor, or a `min` for `web` or `linux`, fails `flui build` and `flui run` with an error naming both values.
- `flui build` packages the floor everywhere it builds: the APK's `minSdk` and the NDK level, `IPHONEOS_DEPLOYMENT_TARGET` and `MACOSX_DEPLOYMENT_TARGET`, and `LSMinimumSystemVersion` in the macOS `Info.plist`.
- The application's floor reaches the start-up refusal: `flui-native`'s build script reads `FLUI_APP_MIN_<ENVIRONMENT>`, which the CLI sets, and the platform refuses to start below the higher of the two floors.

### Changed

- The scaffolded Android `app/build.gradle.kts` reads `minSdk` from the `flui.minSdk` Gradle property, which `flui build` writes into `gradle.properties`. A literal `minSdk` that differs fails the build.
