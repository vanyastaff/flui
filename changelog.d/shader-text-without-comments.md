### Changed

- `flui-engine` links its shader text without comments, indentation or blank lines: `build.rs`
  strips each WGSL source before a pipeline includes it. A release counter is 0.32 MB smaller on
  Linux x86_64 and 0.35 MB smaller on wasm32.
