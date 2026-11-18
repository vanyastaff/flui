### Added

- **Capability crates** (`xtask`, `deny.toml`): `cargo xtask workspace` refuses a capability crate
  with a build script, either a root `build.rs` that `build = false` does not switch off or a
  `build = "<path>"` (ADR-0154 decision 6): a capability crate's Java, Kotlin, Swift and
  Objective-C sources and its dex are built by `flui build` from the sources it declares.
  `deny.toml` bans `android-build` and `swift-rs`, which run `javac`, `d8` or `swift build` from a
  build script.
