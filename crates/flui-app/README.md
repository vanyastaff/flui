# flui-app

**The application layer — where the trees meet the platform.**

`flui-app` is the top of the framework stack: it owns the `run_app` entry
point, constructs an owner-affine `UiRuntime`, hosts the process services still
being extracted by ADR-0027, and drives the frame loop that turns platform
callbacks into build → layout → paint → composite passes.

Part of the [FLUI](https://github.com/vanyastaff/flui) workspace — pre-release,
consumed by path (not published to crates.io).

## How it fits the pipeline

```text
run_app(view)                       — bootstrap: window, GPU surface, frame loop
    │
    ▼
UiRuntime (owner-affine, !Send + !Sync)
    ├── WidgetsBinding              — View → Element, BuildOwner, GlobalKey scope
    ├── GestureBinding              — single-presentation pointer state
    ├── RenderingBinding     — render-view registry, first-frame gate
    └── UpdateScheduler             — frame callbacks, animation tickers (flui-scheduler);
                                      one fresh instance per UI runtime, never shared

PresentationState (per-window, private to flui-app)
    ├── FocusManager                — keyboard event dispatch
    ├── PipelineOwner               — layout/paint (flui-rendering)
    └── SemanticsHost               — enablement + announce/event delivery (flui-app)

AppRuntime (loop-scoped composition root)
    ├── SharedEngineServices        — painting/accessibility, resolved once per owner thread
    ├── frame-wake + platform clipboard
    └── RuntimeRegistry               — any number of UiRuntimeId-keyed UI runtimes
```

- **Entry points** — `run_app` / `run_app_with_config` bootstrap a platform
  window and hand the root `View` to the runner-owned `UiRuntime`, which
  auto-wraps it in an outer `GestureArenaScope` plus `VsyncScope`, so competing
  detectors share the UI runtime arena and implicit-animation widgets tick with
  zero boilerplate.
- **`run_direct`** — an **experimental, direct-engine** escape hatch that
  bypasses the widget tree for raw `SceneBuilder`-callback rendering. It is
  not a supported cross-platform entry point: it does not work on the winit
  backend (the fix is ADR-0039's `on_ready` reorder), has no input handling,
  and does no damage tracking.
- **Lifecycle** — `flui_scheduler::AppLifecycleState` (resumed, inactive,
  hidden, paused, detached) is the canonical state; the
  runner drives `UpdateScheduler::handle_app_lifecycle_state_change` directly at
  bootstrap/shutdown (ADR-0035).
- **Frame loop** — on-demand rendering: a frame runs only when the tree is
  dirty or the scheduler has pending work; physical pacing between frames
  comes from the platform's present path — the blocking Fifo present on
  Vulkan/Wayland, the display-pass cadence on native AppKit (ADR-0058) — not
  from the scheduler itself — `UpdateScheduler` makes no refresh-rate
  assumption of its own.
- **Embedder** (`embedder`) — adapter types connecting the framework to
  windowing, GPU, and input on desktop (Win32/AppKit/headless via
  flui-platform + wgpu); Android/iOS/Web entry points are feature-gated.

This crate deliberately owns **no design tokens**. Colours, typography,
spacing, radius, and motion belong to `flui-material` / `flui-cupertino`;
appearance is per-presentation (`MediaQueryData::platform_brightness`) and the
resolved theme is published by an in-tree inherited widget. See
[ADR-0042](../../docs/adr/ADR-0042-theming-ownership.md); the app-shell widgets
that implement it are tracked in issue #573.

## Known architectural debt

Singleton retirement is complete: `WidgetsBinding`, `GestureBinding`,
`RenderingBinding`, `UpdateScheduler`, and GlobalKey identity are all
UI runtime-owned now — `AppBinding` is deleted, not slimmed, and no test needs a
serialization guard against shared binding state any more (each test
constructs its own independent UI runtime). `AppRuntime` now hosts any number of
`UiRuntimeId`-keyed UI runtimes, and each `UiRuntime` owns an insertion-ordered
presentation forest. `WindowPolicy::Isolated` installs a new UI runtime for a
secondary window; `WindowPolicy::Shared` installs another presentation in
the first hosted UI runtime. The remaining hosting gap is content and rendering:
`open_secondary_window` mounts no root widget, constructs no GPU renderer, and
registers no frame callback for the new window. Production multi-presentation
rendering therefore still needs per-window frame pumps, constraints, sinks,
and submit routing, plus root attachment to a non-primary presentation; that
work remains with issue #559. Logical scheduling (`UpdateScheduler`), physical
pacing, and raster scheduling also remain split across issue #556's remaining
slices. Gesture state is UI runtime-owned but intentionally models one presentation
per UI runtime until that second real presentation consumer exists.

## Documentation

Every public item is documented (`#![deny(missing_docs)]`); build locally with
`cargo doc -p flui-app --open`. Architecture context lives in
[`docs/FOUNDATIONS.md`](../../docs/FOUNDATIONS.md).

## License

MIT OR Apache-2.0, per the workspace license.
