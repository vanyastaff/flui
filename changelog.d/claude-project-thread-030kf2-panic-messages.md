### Added

- `cargo xtask panic-messages`, part of `cargo xtask checks`: a production `expect` or `panic!` whose message neither starts with `BUG: ` nor names a `try_` twin is counted per file in `tools/xtask/allowlists/panic-messages.toml`, and the counts only go down.
