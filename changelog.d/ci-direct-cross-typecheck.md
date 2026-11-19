### Removed

- `cargo xtask cross-typecheck`. CI's `cross-typecheck` job now runs `cargo clippy --workspace --all-targets --target <triple>` for Windows, macOS, Android and iOS, once with default features and once with `--all-features`; the Windows leg runs on Linux through `cargo xwin clippy`. Each leg lints every workspace package, where the xtask command linted only the packages it found native `cfg`s in.
