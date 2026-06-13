# AGENTS.md — flui-foundation

Foundation types and utilities used across the FLUI ecosystem. Minimal dependencies.

## What lives here

- **Tree IDs**: `ViewId`, `ElementId`, `RenderId`, `LayerId`, `SemanticsId` (1-based `NonZeroUsize`, see ID offset pattern in root AGENTS.md)
- **Keys**: `Key`, `ValueKey`, `UniqueKey` for widget identity
- **Change notification**: `ChangeNotifier`, `Listenable` trait, `ListenerId`
- **Callbacks**: `VoidCallback`, `ValueChanged` type aliases
- **Logging**: `flui_foundation::log::Logger` (merged from flui-log). Re-exported by `flui-app`
- **Notifications**: base abstractions for event bubbling

## Key constraints

- **No `println!`/`eprintln!`/`dbg!`** — enforced by port-check trigger #15. Use `tracing` macros
- `ChangeNotifier` uses `SmallVec<4>` for listener snapshots (stack-allocated common case)
- Platform-specific log backends: `android_log-sys` (Android), `tracing-oslog` (iOS), `tracing-wasm` (WASM)
- `pretty` feature enables `tracing-forest` for hierarchical desktop logging

## Architecture doc

See `crates/flui-foundation/ARCHITECTURE.md` for deep architecture.
