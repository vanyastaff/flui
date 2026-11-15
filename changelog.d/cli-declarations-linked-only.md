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
- A git dependency on a branch or tag whose name holds `/` or `+`, a sparse registry and two
  registries' crates of one name and version no longer fail every `flui` command that reads
  declarations.
- `flui build ios` reads the declarations of the package it built, not of every member of the
  workspace, and `flui build android` and `flui doctor` those of what cargo builds by default in
  the project's directory: the package there, or at a workspace's root its default member, also
  when the root is a package itself. A root with several default members is refused by name
  instead of joining several applications' declarations, and the package built is no longer
  refused as a dependency when a crate of its build depends on it.
- `flui run` checks the package `cargo run` builds, with the features it builds with (an
  optimized `--agent` run's driver feature included), and `flui build` selects the default
  members of a workspace root that is a package itself, as cargo does.
- `flui add` prints each platform's declarations from that platform's build on any host, and
  reads the crate the application's dependency resolves to, not another of its name.
