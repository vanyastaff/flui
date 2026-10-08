### Fixed

- Native type-checking selects target-gated workspace packages directly, including their tests, examples and benches, instead of relying on a fixed package list and indirect library builds.
- Native type-checking supports `--all-features`; CI compiles optional native paths as well as the required-target configuration, using Xcode SDK hosts for both Apple targets.
- Enabling every platform feature on mobile no longer pulls in the desktop winit clipboard backend.
- The CLI's shared browser device schema compiles on mobile hosts where desktop browser probes are unavailable.
- TOML formatting excludes local `.10x` tool artifacts, including downloaded SDK dependencies.
