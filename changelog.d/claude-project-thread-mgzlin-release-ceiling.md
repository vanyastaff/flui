### Added

- **The facade's `release-max-level-info` feature** (ADR-0398): `tracing`'s `release_max_level_info`,
  through `flui-app` and `flui-log`. A build without debug assertions holds no `trace!` or `debug!`
  call site, FLUI's or the application's: about 0.7 MB of a generated counter on Linux x86_64 and
  0.45 MB on wasm32. The subscriber `flui_log` installs over a filter that selects events above
  the build's ceiling reports it once, as a warning on target `flui_log::ceiling` through the
  platform's sink, and `flui_log::ResilientSetup::compiled_out` names the level.

### Changed

- **`flui build --release` and `flui run --release` turn the release ceiling on** for every platform,
  through the key under which the built package names the facade (or `flui-app`), and
  `flui run --profile release` is `--release` on every target. A debug build, any other
  `--profile` and a `flui run --agent` build keep every span, and so does a plain
  `cargo build --release`: an application that ships through cargo alone passes
  `--features flui/release-max-level-info` itself. A facade older than this feature fails cargo's
  resolution of a `--release` build: update it with the CLI.
