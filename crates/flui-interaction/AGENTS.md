# AGENTS.md — flui-interaction

Event routing, hit testing, focus management, and gesture recognition.

## What lives here

- **EventRouter** — routes pointer/keyboard events via hit testing
- **HitTest** — spatial hit testing (sealed `HitTestable` trait)
- **FocusManager / FocusScope** — keyboard focus (one presentation-owned `Rc` manager, no global/TLS fallback, `FocusTraversalPolicy`)
- **GestureArena** — resolves conflicts between competing gesture recognizers
- **GestureBinding arena lifecycle** — production bindings use a shared `BindingDriven` arena: dispatch first, close on Down, sweep on Up, never force-sweep Cancel
- **GestureRecognizers** — Tap, Drag, Scale, LongPress, ForcePress
- **PointerEventResampler** — raw input resampling
- **VelocityTracker** — gesture velocity computation (LSQ solver)

## Key constraints

- **Sealed traits** — `HitTestable` and `GestureArenaMember` cannot be implemented outside this crate. API evolution without breaking changes.
- **`testing` feature** — gates `testing/` submodule (gesture recording, replay, builders) + `PointerEventData`/`make_*_event` helpers. Auto-enabled via `cfg(any(test, feature = "testing"))`.
- **4 benchmarks** — `velocity_tracker_bench`, `gesture_arena_bench`, `tap_detector_bench`, `pointer_resampler_bench`. Bench fixtures use `testing` feature helpers.
- **`PointerId`** — re-exported from `ui-events` crate (`NonZeroU64`-backed). `FocusNodeId` and `HandlerId` are crate-local `NonZeroU64` newtypes.
- **Async dependency** — `tokio` with `time`, `sync`, `macros`, `rt` features. Used for gesture timing.
- **Owner-local callbacks** — gesture/focus handlers may capture `Rc` state;
  data-plane IDs and hit paths stay `Send + Sync`.
- **Hosted-node cleanup** — use `FocusNodeRegistration` for widget-installed
  key handlers and rect providers; its generation check prevents a stale host
  from erasing a newer external writer.
- **Property tests** — `proptest` for LSQ solver and velocity tracker math substrate.
