### Added

- `[package.metadata.flui.declares.android] proguard`: a crate's R8 rules. `flui build` hands them to
  a Gradle release as `flui.proguardFiles`, which the scaffolded `app/build.gradle.kts` adds to its
  release rules; `flui-native` declares rules that keep `dev.flui.**`, so a minified release keeps
  the classes the native library calls by name. A build script that does not read the property is
  refused.

### Fixed

- `flui create` scaffolds the `app/proguard-rules.pro` the Android build script names.
