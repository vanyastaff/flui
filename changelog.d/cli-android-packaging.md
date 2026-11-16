### Added

- `flui.toml` takes `[signing.android]` (`keystore`, `alias`): `flui build android --release` signs the APK with that key on both packagers, its passwords read from `FLUI_ANDROID_KEYSTORE_PASSWORD` and `FLUI_ANDROID_KEY_PASSWORD`, never from a file or the command line. A release of an application that names no key is signed with the debug key, and the build warns that a store refuses it.
- The APK states `[app] version`: `versionName` as written, `versionCode` as `major * 10000 + minor * 100 + patch`.
- `flui run --scene` takes `--device`, and `--package` defaults to `flui.toml`'s `[app] id`.

### Changed

- `[app] version` must be SemVer: `flui.toml` with another version is refused when it loads. An Android build refuses a version whose minor or patch is above 99.
- `[platforms.android] min` above the `targetSdk` FLUI declares is refused.
- A Gradle project's `app/build.gradle.kts` reads `flui.versionCode`, `flui.versionName`, `flui.ndkPath` and, for a release, `flui.signing.storeFile` from `gradle.properties`, as `flui create` now scaffolds it; a script with a literal version that differs, an `ndkVersion`, or (for a release) no signing config is refused with the line to write.
- Both Android packagers write `<name>-<profile>.apk`; the packager without Gradle no longer leaves its unsigned intermediate APKs in the output directory.
- The Rust target checks of the Android, iOS and web builds ask `rustc --print target-libdir` under a deadline instead of `rustup target list`, so a toolchain without rustup is read too.

### Fixed

- A Gradle release build no longer copies an unsigned APK that `adb install` refuses: the build fails and names it.
- `cargo ndk` links with the NDK `flui build` resolved and prints, with versions under the SDK's `ndk/` compared by number, and a Gradle project strips with the same NDK.
- The packager without Gradle signs debug builds with the debug keystore the Android Gradle plugin uses (`ANDROID_USER_HOME`, then `.android` under `USERPROFILE` on Windows), so its APKs install over Gradle's.
- The packager without Gradle states `android:extractNativeLibs="false"`, so the stored libraries are mapped from the APK instead of extracted.
- `flui run --scene` copies its plugin into the application: the device's shell no longer receives `sh -c cp` without operands, the push names the device by serial, and the library is the one cargo-ndk built for the scene crate.
