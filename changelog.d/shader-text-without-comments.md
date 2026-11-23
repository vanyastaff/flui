### Changed

- `flui-engine` links its shader text without comments, indentation or blank lines: `build.rs`
  strips each WGSL source before a pipeline includes it. A release application is about 0.2 MB
  smaller on every target.
