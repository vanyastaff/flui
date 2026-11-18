### Added

- **Build scripts** (`xtask`, `deny.toml`): `cargo xtask workspace` fails a workspace member whose
  build script, or a file it mounts, spawns a platform compiler or packager (`javac`, `kotlinc`,
  `d8`, `r8`, `swiftc`, `xcrun`, `xcodebuild`, `gradlew`, …), named directly, by a path, or
  through a constant, binding or function of the script; a comment or a `cargo:` line naming one
  is not a finding. `deny.toml` bans `android-build` and `swift-rs`, which run those tools from a
  build script. Java, Kotlin, Swift and dex code is built by `flui build` from the sources a crate
  declares.
