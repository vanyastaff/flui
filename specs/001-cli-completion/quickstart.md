# Quickstart: flui-cli Completion

**Branch**: `001-cli-completion` | **Date**: 2026-02-08

## Prerequisites

- Rust 1.91+ with `cargo`
- FLUI workspace checked out (`crates/flui-cli/` exists)
- For emulator tests: Android SDK (optional) and/or Xcode on macOS (optional)

## Development Setup

```bash
# Ensure you're on the feature branch
git checkout 001-cli-completion

# Enable flui-cli in workspace (uncomment in Cargo.toml)
# "crates/flui-cli",

# Build the CLI
cargo build -p flui-cli

# Run the CLI
cargo run -p flui-cli -- --help
```

## Implementation Order

Work in this order to minimize blocked dependencies:

1. **Templates fix** (FR-010, FR-011, FR-012) — unblocks integration tests
2. **Platform add/remove** (FR-006..FR-009) — self-contained, uses existing config
3. **Emulators command** (FR-001..FR-005) — self-contained, external tool parsing
4. **Integration tests** (FR-013..FR-015) — uses all above
5. **DevTools** (FR-016, FR-017) — depends on flui-devtools feature flag
6. **Hot reload** (FR-018, FR-019) — new dependency, most complex

## Key Files to Modify

| File | Change Type | Description |
|------|-------------|-------------|
| `Cargo.toml` (workspace) | Uncomment | Re-enable `crates/flui-cli` |
| `crates/flui-cli/Cargo.toml` | Add deps | `notify`, `notify-debouncer-mini`, optional `flui-devtools` |
| `crates/flui-cli/src/main.rs` | Modify | Emulators subcommand structure, `--local` flag on create |
| `crates/flui-cli/src/commands/emulators.rs` | Rewrite | Full emulator management |
| `crates/flui-cli/src/commands/platform.rs` | Modify | Implement `add()` and `remove()` |
| `crates/flui-cli/src/commands/devtools.rs` | Rewrite | Conditional devtools launch |
| `crates/flui-cli/src/commands/run.rs` | Modify | Add file watcher for hot reload |
| `crates/flui-cli/src/templates/basic.rs` | Modify | Fix dependency declarations |
| `crates/flui-cli/src/templates/counter.rs` | Modify | Fix dependency declarations |
| `crates/flui-cli/tests/*.rs` | New files | Integration test suite |

## Testing

```bash
# Run unit tests
cargo test -p flui-cli

# Run integration tests (requires built binary)
cargo test -p flui-cli --test cli_create
cargo test -p flui-cli --test cli_errors

# Run all tests including platform-specific ones
cargo test -p flui-cli --features platform-tests

# Lint check
cargo clippy -p flui-cli -- -D warnings
```

## Verification Checklist

After implementation, verify:

- [ ] `flui create my-app` generates a project that passes `cargo check`
- [ ] `flui create my-app --local` generates path-based dependencies
- [ ] `flui emulators list` shows available emulators (or helpful error if no SDK)
- [ ] `flui emulators launch <name>` starts an emulator
- [ ] `flui platform add android` creates directory and updates `flui.toml`
- [ ] `flui platform remove android` removes with confirmation prompt
- [ ] `flui devtools` shows instructions (without devtools feature)
- [ ] `flui run --hot-reload` watches files and rebuilds on change
- [ ] `cargo test -p flui-cli` passes all tests
- [ ] `cargo clippy -p flui-cli -- -D warnings` has no warnings
- [ ] Test coverage >= 70%
