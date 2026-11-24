# flui-devtools

Runtime developer tooling for FLUI: the half that runs inside the application. Small,
feature-gated modules, each an adapter over a seam the framework already exposes. Nothing here
walks a widget, element or render tree itself, opens an endpoint, or watches files. A release
build stays at zero devtools cost by not depending on this crate.

| Module | Feature | What it is |
|--------|---------|------------|
| `capture` | `profiling` | `Capture`, the one recorder of the framework's frame spans and frame telemetry |
| `profiler` | `profiling` | `Profiler`: per-frame phase timings, jank detection, frame rate and history |
| `timeline` | `timeline` | `Timeline`: an event recorder with Chrome trace and JSON export |
| `inspector` | `inspector` | `InspectorCounters`, a counting `TreeObserver` |
| `gesture_inspector` | `inspector` | `GestureInspector`, a per-contest report of a gesture arena |
| `gesture_overlay` | `inspector` | `GestureInspectorOverlay`, a debug overlay drawing live contests |

The first three features are on by default.

## Profiling

```rust
use flui_devtools::{Capture, Profiler, ProfilerConfig};
use flui_log::config::LogConfig;
use flui_log::ownership::{InstallPolicy, LogBridgePolicy, install_subscriber};
use tracing_subscriber::layer::SubscriberExt;

let capture = Capture::new();
let subscriber = LogConfig::default().subscriber()?.with(capture.layer());
install_subscriber(subscriber, InstallPolicy::Auto, LogBridgePolicy::Auto)?;
let profiler = Profiler::over(&capture, ProfilerConfig::default());

// run the app, then read `profiler.frame_stats()` and `profiler.frame_rate()`
```

To count mounts, rebuilds and unmounts, install an `InspectorCounters` with
`AppConfig::dev_tree_observer`.

## More

- `cargo doc -p flui-devtools --open` has the crate documentation: what a capture reads, build
  levels, animation timing, jank and frame rate, the timeline export and the inspector.
- [ARCHITECTURE.md](ARCHITECTURE.md) states the invariants each module holds and the tests that
  pin them.

## License

MIT OR Apache-2.0
