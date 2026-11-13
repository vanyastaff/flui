### Changed

- Upgrade the pinned development toolchain to Rust 1.100.0 and the workspace and generated-project MSRV to Rust 1.100.
- Migrate deprecated atomic updates to `try_update` and adapt test assertions, lint expectations and compile-fail diagnostic fixtures to Rust 1.99.
- Replace `map(…).unwrap_or_default()` with `map_or_default` and drop redundant clones that clippy 1.100 reports.
