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
- **`flui build` must enter a tokio runtime before driving any `flui-build`
  builder.** The builders shell out through `tokio::process`; `pollster` drives
  the future but installs no reactor, so without the runtime every build path
  panicked "there is no reactor running". See `commands/build.rs::execute`.
- **`--example`/`--package` select the cargo unit.** The FLUI source tree's
  runnable entry points are examples, not a binary package, so a bare
  `flui build desktop` there is refused with a message naming the flag
  (`ensure_resolvable_target`). A generated project needs neither.
- **`flui create --hot-reload` emits a three-crate *workspace*** (types +
  logic + host), not one crate. `tests/cli_create.rs` gates it with a real
  `cargo check --workspace`, same rule as the single-crate templates.
- **`flui build macos` stages a `.app`**, reading identity from `flui.toml`'s
  `[app]` section. `flui build macos --universal` builds both darwin slices and
  fuses them with `lipo`.
- **`flui build ios` shells out to `IOSBuilder`** and builds the current
  package's static library for `aarch64-apple-ios` (or both slices with
  `--universal`). It is not the deprecated "not yet supported" message it once
  printed.
- **Worker hot reload stages at a content-addressed path.** `flui run`'s worker
  mode does not load `target/<profile>/lib<worker>.dylib` directly; it copies
  each build to `{stem}-hot-{fnv1a-hash}{ext}` (hash over the built bytes) and
  writes that path into the `.flui_worker_plugin` sidecar. The changing name is
  what defeats macOS's deferred-unmap dyld (a same-path reload can serve the
  retained image). Identical bytes reuse the file untouched; superseded versions
  are pruned best-effort. Do **not** reintroduce fixed A/B staging slots — they
  stop changing path once both exist. The initial launch and every restart also
  load a staged copy (never the canonical file), so cargo can overwrite the
  canonical output on every platform while the host holds the staged one mapped.
  On macOS each staged dylib is ad-hoc `codesign`ed best-effort (defensive; cargo
  already linker-signs). See `stage_worker_artifact` in `src/commands/run.rs`.
- **The worker and host must be built in ONE cargo invocation.** `flui run`
  always builds them together (`cargo build -p worker -p host`), because cargo
  unifies dependency features **per invocation** and Rust's `TypeId` is stable
  only within one compiled instance of a crate. Building the worker separately
  (or into an isolated `--target-dir`) can give the worker a different instance
  of `flui-widgets`/`flui-view` than the host; the worker's
  `TypeId::of::<GestureArenaScope>()` then misses the host's `HashMap<TypeId,
  ElementId>`, and the first frame panics with "gesture consumers must be mounted
  beneath GestureArenaScope". This is why `run_cargo_build_packages` takes a
  slice of packages and why the old `worker-isolated-target-dir` scheme is gone.
  Do not split the build back into per-package calls.
