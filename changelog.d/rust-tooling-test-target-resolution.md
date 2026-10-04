### Fixed

- The workspace test-reachability check uses Cargo's resolved test target paths and parsed Rust module declarations. Implicit named targets are recognized; comments and string literals cannot hide unmounted test files.
