# AGENTS.md — flui-build

Build system library for cross-platform FLUI builds (Android, Web/WASM, iOS, Desktop).

**Status:** Not in workspace `default-members`. Build explicitly with `cargo build -p flui-build`.

## What lives here

- **`PlatformBuilder` trait** — common interface for all platform builders
- **`AndroidBuilder`** — builds APKs via cargo-ndk + Gradle
- **`WebBuilder`** — builds WASM packages via wasm-pack
- **`DesktopBuilder`** — builds native desktop applications
- **`IOSBuilder`** — builds iOS static libraries (`lib{crate}.a`), optionally through Xcode
- **`BuilderContext`** — build configuration (platform, `BuildTarget`, profile, features, output dir, optional `AppBundle`)

## Selecting *what* to build

`BuilderContext.target: BuildTarget` names the cargo unit — `DefaultBinary`
(the current directory's own binary, the generated-project case),
`Package(name)`, or `Example(name)`. **A builder must never hard-code the
manifest it compiles.** `DesktopBuilder` and `IOSBuilder` used to point at
`crates/flui_app/Cargo.toml` and take `libflui_app.*`, which cannot exist in a
generated project and produced the framework library instead of the user's
executable. Set `--example`/`--package` on the CLI to choose; `DefaultBinary`
reads the current manifest's `[package].name` / `[[bin]].name`.

## macOS `.app` staging

`DesktopBuilder::build_platform` stages a `.app` when `ctx.bundle` is `Some`
and the host is macOS; otherwise it copies the bare executable.
`AppBundle::new(name, prefix)` derives the identifier. `flui build macos`
supplies the metadata from `flui.toml`'s `[app]` section (directory name as
fallback). The `.app` is what makes the result a launchable application rather
than a loose Mach-O.

## Key constraints

- **Async** — uses `tokio` with `process` feature for spawning build tools.
  **The caller must enter a tokio runtime.** `pollster::block_on` drives the
  future but installs no reactor, so `tokio::process` panics "there is no
  reactor running" — `flui build` enters a multi-thread runtime before driving
  any builder. A new caller must do the same.
- **Dependencies** — `which` (tool discovery), `indicatif` (progress bars),
  `serde_json`, `toml` (reading a manifest's binary name), `thiserror`
- **Dev dependency** — `tempfile` for test build directories
- **`--locked`** — nothing here; the builders shell out and the lock discipline
  belongs to the invoking command
