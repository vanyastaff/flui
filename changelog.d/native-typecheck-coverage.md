### Fixed

- Native type-checking selects target-gated workspace packages directly, including their tests, examples and benches, instead of relying on a fixed package list and indirect library builds.
- The CLI's shared browser device schema compiles on mobile hosts where desktop browser probes are unavailable.
- TOML formatting excludes local `.10x` tool artifacts, including downloaded SDK dependencies.
