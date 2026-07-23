# AGENTS.md — flui-interaction

Event routing, hit testing, focus management, and gesture recognition.

## What lives here

- **EventRouter** — routes pointer/keyboard events via hit testing
- **HitTest** — spatial hit testing (sealed `HitTestable` trait)
- **FocusManager / FocusScope** — keyboard focus (owner-thread TLS manager, `FocusTraversalPolicy`)
- **GestureArena** — resolves conflicts between competing gesture recognizers
- **GestureRecognizers** — Tap, Drag, Scale, LongPress, ForcePress
- **PointerEventResampler** — raw input resampling
- **VelocityTracker** — gesture velocity computation (LSQ solver)

## Key constraints

- **Sealed traits** — `HitTestable` and `GestureArenaMember` cannot be implemented outside this crate. API evolution without breaking changes.
- **`testing` feature** — gates `testing/` submodule (gesture recording, replay, builders) + `PointerEventData`/`make_*_event` helpers. Auto-enabled via `cfg(any(test, feature = "testing"))`.
- **4 benchmarks** — `velocity_tracker_bench`, `gesture_arena_bench`, `tap_detector_bench`, `pointer_resampler_bench`. Bench fixtures use `testing` feature helpers.
- **`PointerId`** — re-exported from `ui-events` crate (`NonZeroU64`-backed). `FocusNodeId` and `HandlerId` are crate-local `NonZeroU64` newtypes.
- **Async dependency** — `tokio` with `time`, `sync`, `macros`, `rt` features. Used for gesture timing.
- **`dashmap`** — concurrent hash map for handler registry.
- **Property tests** — `proptest` for LSQ solver and velocity tracker math substrate.
