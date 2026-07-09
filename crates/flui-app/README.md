# flui-app

**The application layer — where the three trees meet the platform.**

`flui-app` is the top of the framework stack: it owns the `run_app` entry
point, combines every subsystem binding into one `AppBinding` singleton, and
drives the frame loop that turns platform callbacks into build → layout →
paint → composite passes, mirroring Flutter's binding architecture:

| FLUI | Flutter |
|------|---------|
| `AppBinding` (alias `WidgetsFlutterBinding`) | `WidgetsFlutterBinding` |
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
AppBinding (central coordinator)    — singleton combining all bindings
    ├── GestureBinding              — pointer events, hit testing, event coalescing
    ├── FocusManager                — keyboard event dispatch
    ├── WidgetsBinding              — build phase (View → Element, flui-view)
    ├── RenderingFlutterBinding     — layout/paint via PipelineOwner (flui-rendering)
    ├── SemanticsBinding            — accessibility tree flushes (flui-semantics)
    └── Scheduler                   — frame callbacks, animation tickers (flui-scheduler)
```

- **Entry points** — `run_app` / `run_app_with_config` bootstrap a platform
  window and hand the root `View` to `AppBinding::attach_root_widget`, which
  auto-wraps it in `VsyncScope` so implicit-animation widgets tick with zero
  boilerplate. `run_direct` bypasses the widget tree for raw
  `SceneBuilder`-callback rendering.
- **Lifecycle** — `LifecycleState`/`LifecycleEvent` port Flutter's
  `AppLifecycleState` (resumed, inactive, paused, detached);
  `DefaultLifecycle` is the stock observer.
- **Frame loop** — on-demand rendering: a frame runs only when the tree is
  dirty or the scheduler has pending work, and pure ticker-driven frames are
  paced to the configured target FPS.
- **Embedder** (`embedder`) — adapter types connecting the framework to
  windowing, GPU, and input on desktop (Win32/AppKit/headless via
  flui-platform + wgpu); Android/iOS/Web entry points are feature-gated.
- **Theme** (`theme`) — `AppTheme` pre-tree configuration with semantic
  `ColorScheme` tokens; distinct from the in-tree `flui_widgets::Theme`
  inherited widget.

## Known architectural debt

`AppBinding`, `Scheduler`, and friends are process-wide singletons
(`instance()`), mirroring Flutter's `WidgetsFlutterBinding.instance`. Tests
share that global, so any test mutating binding state must serialize against
the others touching the same field — `renderer_binding.rs` does this with
`SEMANTICS_TEST_LOCK` around the `semantics_enabled` toggles. Under `cargo
nextest` each test gets its own process, so the tests run fully parallel;
under `cargo test` they share one process and rely on that lock.

Scoping binding state per test/app instance (via `flui-binding`'s
`HeadlessBinding`) remains a tracked ship-quality item, but it no longer
gates parallel test execution.

## Documentation

Every public item is documented (`#![deny(missing_docs)]`); build locally with
`cargo doc -p flui-app --open`. Architecture context lives in
[`docs/FOUNDATIONS.md`](../../docs/FOUNDATIONS.md).

## License

MIT OR Apache-2.0, per the workspace license.
