### Added

- **The facade's `release-max-level-info` feature** (ADR-0398): `tracing`'s `release_max_level_info`,
  through `flui-app` and `flui-log`. A build without debug assertions holds no `trace!` or `debug!`
  call site, FLUI's or the application's: about 0.7 MB of a generated counter on Linux x86_64 and
  0.45 MB on wasm32. `flui_log::ResilientSetup::compiled_out` names the level a filter selects
  above the build's ceiling, and the managed entry point reports it on standard error.

### Changed

- **`flui build --release` and `flui run --release` turn the release ceiling on** for every platform,
  through the key under which the built package names the facade (or `flui-app`). A debug build, a
  named `--profile` and a `flui run --agent` build keep every span; a plain `cargo build --release`
  does too, so an application that ships through cargo alone passes
  `--features flui/release-max-level-info` itself. A facade older than this feature fails cargo's
  resolution of a `--release` build: update it with the CLI.
