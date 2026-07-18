# flui-app

**The application layer — where the three trees meet the platform.**

`flui-app` is the top of the framework stack: it owns the `run_app` entry
point, constructs an owner-affine `UiRealm`, hosts the process services still
being extracted by ADR-0027, and drives the frame loop that turns platform
callbacks into build → layout → paint → composite passes. FLUI preserves
Flutter's tree behavior without copying its process/runtime topology:

| FLUI | Flutter |
|------|---------|
| `UiRealm` + transitional `AppBinding` | `WidgetsFlutterBinding` |
| `run_app` / `run_app_with_config` | `runApp` |
| `WidgetsBinding` | `WidgetsBinding` |
| `RenderingFlutterBinding` + `PipelineOwner` | `RendererBinding` |
| `GestureBinding` / `PaintingBinding` / `SemanticsBinding` / `Scheduler` | `GestureBinding` / `PaintingBinding` / `SemanticsBinding` / `SchedulerBinding` |

Part of the [FLUI](https://github.com/vanyastaff/flui) workspace — pre-release,
consumed by path (not published to crates.io).

## How it fits the pipeline

```text
run_app(view)                       — bootstrap: window, GPU surface, frame loop
    │
    ▼
UiRealm (owner-affine, !Send + !Sync)
    └── WidgetsBinding              — View → Element, BuildOwner, GlobalKey scope

AppBinding (transitional host)
    ├── GestureBinding              — pointer events, hit testing, event coalescing
    ├── FocusManager                — keyboard event dispatch
    ├── RenderingFlutterBinding     — layout/paint via PipelineOwner (flui-rendering)
    ├── SemanticsBinding            — accessibility tree flushes (flui-semantics)
    └── Scheduler                   — frame callbacks, animation tickers (flui-scheduler)
```

- **Entry points** — `run_app` / `run_app_with_config` bootstrap a platform
  window and hand the root `View` to the runner-owned `UiRealm`, which
  auto-wraps it in `VsyncScope` so implicit-animation widgets tick with zero
  boilerplate. `run_direct` bypasses the widget tree for raw
  `SceneBuilder`-callback rendering.
- **Lifecycle** — `flui_scheduler::AppLifecycleState` (resumed, inactive,
  hidden, paused, detached) is the canonical Flutter-parity state; the
  runner drives `Scheduler::handle_app_lifecycle_state_change` directly at
  bootstrap/shutdown (ADR-0035).
- **Frame loop** — on-demand rendering: a frame runs only when the tree is
  dirty or the scheduler has pending work, and pure ticker-driven frames are
  paced to the configured target FPS.
- **Embedder** (`embedder`) — adapter types connecting the framework to
  windowing, GPU, and input on desktop (Win32/AppKit/headless via
  flui-platform + wgpu); Android/iOS/Web entry points are feature-gated.
- **Theme** (`theme`) — `AppTheme` pre-tree configuration with semantic
  `AppColorScheme` tokens; distinct from the in-tree `flui_material::Theme`
  inherited widget.

## Known architectural debt

`WidgetsBinding` and GlobalKey identity are realm-owned. `AppBinding`,
`Scheduler`, renderer orchestration, and some gesture/focus services remain
process-scoped migration debt. Tests mutating those remaining globals must
use their existing serialization guard; the guard retires as each service
moves behind an explicit owner.

## Documentation

Every public item is documented (`#![deny(missing_docs)]`); build locally with
`cargo doc -p flui-app --open`. Architecture context lives in
[`docs/FOUNDATIONS.md`](../../docs/FOUNDATIONS.md).

## License

MIT OR Apache-2.0, per the workspace license.
