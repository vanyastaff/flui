### Fixed

- `flui build` declares in the manifests it writes only what the built artifact links. It used
  to join the declarations of every crate `cargo metadata` resolves, with the features that
  graph unifies, so an Android permission, a privacy reason, a URL scheme or a file type could
  come from a dev-dependency, from a feature only a dev-dependency turns on, from a build
  script's dependency or from a procedural macro's own copy of a crate. An application without
  the facade's `http` no longer ships `android.permission.INTERNET` because a test of its
  workspace turns `http` on. The Android manifest, the iOS `Info.plist` and privacy manifest, the
  macOS bundle, the Linux desktop entry, the one-instance check, `flui doctor` and `flui add`
  read the same crates.
- `flui build ios` reads the declarations of the package it built, not of every member of the
  workspace, and `flui build android` and `flui doctor` those of what cargo builds by default in
  the project's directory.
