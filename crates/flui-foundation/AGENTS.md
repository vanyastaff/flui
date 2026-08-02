# AGENTS.md — flui-foundation

Foundation types and utilities used across the FLUI ecosystem. Minimal dependencies.

## What lives here

- **Tree IDs**: `ViewId`, `ElementId`, `RenderId`, `LayerId`, `SemanticsId` (1-based `NonZeroUsize`, see ID offset pattern in root AGENTS.md)
- **Keys**: `Key`, `ValueKey`, `UniqueKey` for widget identity
- **Change notification**: `ChangeNotifier`, `Listenable` trait, `ListenerId`
- **Callbacks**: `VoidCallback`, `ValueChanged` type aliases
- **Diagnostic vocabulary**: `diagnostics` — durable structured `tracing` field names already emitted across crate boundaries. Today this includes `presentation_id`; internal runtime topology is deliberately not part of the schema
- **Notifications**: base abstractions for event bubbling

## Key constraints

- **No `println!`/`eprintln!`/`dbg!`** — enforced by port-check trigger #15. Use `tracing` macros
- `ChangeNotifier` uses `SmallVec<4>` for listener snapshots (stack-allocated common case)
- **Emission only, never installation.** Foundation depends on `tracing` and has *no* dependency on `tracing-subscriber`, `tracing-forest`, or any OS log sink — that is what lets an embedded host link foundation without linking a subscriber backend at all. Adding one back is the boundary erosion issue #568 undid. Subscriber construction lives in `flui-log` (`crates/flui-log/AGENTS.md`), which only composition roots may depend on.

## Architecture doc

See `crates/flui-foundation/ARCHITECTURE.md` for deep architecture.
