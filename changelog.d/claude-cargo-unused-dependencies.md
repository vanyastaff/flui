### Changed

- `cargo xtask deps` runs cargo-deny alone: cargo 1.100's `unused_dependencies` lint, fatal in every CI build, reports what cargo-shear reported (an unused dependency, a test-only one declared as normal, an unused `[workspace.dependencies]` entry), per feature and target, and cargo-shear and its `[package.metadata.cargo-shear]` ignores are gone.
