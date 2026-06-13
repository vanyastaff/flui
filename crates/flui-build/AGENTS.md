# AGENTS.md — flui-build

Build system library for cross-platform FLUI builds (Android, Web/WASM, iOS, Desktop).

**Status:** Not in workspace `default-members`. Build explicitly with `cargo build -p flui-build`.

## What lives here

- **`PlatformBuilder` trait** — common interface for all platform builders
- **`AndroidBuilder`** — builds APKs via cargo-ndk + Gradle
- **`WebBuilder`** — builds WASM packages via wasm-pack
- **`DesktopBuilder`** — builds native desktop applications
- **`BuilderContext`** — build configuration (platform, profile, targets)

## Key constraints

- **Async** — uses `tokio` with `process` feature for spawning build tools
- **Dependencies** — `which` (tool discovery), `indicatif` (progress bars), `serde_json`, `thiserror`
- **Dev dependency** — `tempfile` for test build directories
