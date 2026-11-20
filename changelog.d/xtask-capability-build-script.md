### Added

- **Capability crates**: a capability crate declares no build script unless it exports a global
  native symbol (`links`), either a root `build.rs` that `build = false` does not switch off or a
  `build = "<path>"` (ADR-0154 decision 6): its Java, Kotlin, Swift and Objective-C sources and
  its dex are built by `flui build` from the sources it declares. A reader of a change holds this
  rule (AGENTS.md, "Rules a reader holds"). `deny.toml` bans `android-build` and `swift-rs`, which
  run `javac`, `d8` or `swift build` from a build script.
