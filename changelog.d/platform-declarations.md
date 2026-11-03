### Added

- A crate states what it needs from an application's manifests in
  `[package.metadata.flui.declares]` (`schema = 1`, `android.permissions`, and per Cargo feature
  under `features.<feature>`). `flui build` joins the declarations of every crate the target links
  and writes them into the manifests it generates; an unknown key, a newer schema or a malformed
  value fails the build naming the crate.

### Changed

- `flui build` generates the Android `AndroidManifest.xml` whole on every build, from FLUI's base,
  the crates' declarations and `flui.toml`. A manifest at
  `platforms/android/app/src/main/AndroidManifest.xml` that `flui build` did not write is refused:
  delete it and build again.

### Fixed

- Android applications declare `android.permission.INTERNET` when they link `flui-app`'s `http`
  feature (on by default), so the HTTP capability and `Image::network` no longer fail with a
  `SecurityException` on Android.
