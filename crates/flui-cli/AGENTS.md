# AGENTS.md — flui-cli

CLI tool for the FLUI framework. Ships the `flui` binary.

**Status:** Built out (~7.7k LOC) and included in workspace `default-members`.
Bin-only crate — there is no `src/lib.rs`, so unit tests live in the bin
target (`cargo test -p flui-cli --bins`, not `--lib`).

## What lives here

| Path | Purpose |
|------|---------|
| `src/commands/` | `create` / `build` / `run` / `doctor` / `devices` / `completions` |
| `src/templates/{basic,counter}.rs` | Project scaffolds emitted by `flui create` |
| `tests/cli_create.rs` | `flui create` integration tests |

## Templates — the rules

`flui create` must emit a project that **actually compiles**. File-existence
assertions are not enough: they pass just as happily on a template frozen
against a long-deleted API (this is exactly how the templates rotted into
referencing a `flui_core` crate that never existed here).

`tests/cli_create.rs` therefore runs a real `cargo check` on the generated
output for both templates. When you touch a template:

- The generated project sits at `<repo>/target/<name>` because `--local` emits
  `path = "../../crates/flui-app"` deps, which only resolve exactly one
  directory below the repo root.
- The check uses its own `--target-dir`; do **not** point it at the workspace
  target dir, or it deadlocks on the build lock the outer `cargo test` holds.
- Only `--local` (path deps) resolves today — FLUI is not on crates.io, so the
  published-version branch of `generate_cargo_toml` cannot be compile-tested.

Templates target the **current** public surface, not an aspirational one. No
`MaterialApp`/`Scaffold`/`AppBar` (Catalog.1, unbuilt). If a template cannot
express something through today's public API, emit the honest minimal version
plus a doc comment pointing at the pattern to grow into — never an unverified
template that only passes snapshot tests.

## Gotchas

- Cargo dependency names are hyphenated (`flui-app`). Anything sniffing a
  generated `Cargo.toml` must not match on `flui_app` alone — see
  `has_flui_dependency` in `src/commands/run.rs`.
