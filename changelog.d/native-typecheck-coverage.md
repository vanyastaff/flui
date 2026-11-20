### Fixed

- Enabling every platform feature on mobile no longer pulls in the desktop winit clipboard backend.
- The CLI's shared browser device schema compiles on mobile hosts where desktop browser probes are unavailable.
- TOML formatting excludes local `.10x` tool artifacts, including downloaded SDK dependencies.
