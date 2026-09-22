# flui-devtools

Runtime developer tooling for FLUI: the half that runs inside the
application. Three small, feature-gated modules, each an adapter over a seam
the framework already exposes. Nothing here walks a widget, element or render
tree, opens a port, or watches files.

| Module | Feature | What it is |
|--------|---------|------------|
| `profiler` + `frame_timing_layer` | `profiling` | Per-frame build/layout/paint/compositing timings, jank detection, FPS and history. Fed by a `tracing` layer that subscribes to the framework's own frame spans. |
| `timeline` | `timeline` | An event recorder with Chrome trace (`chrome://tracing`) and JSON export, plus a bridge that turns the scheduler's `FrameSnapshot`s into trace events. |
| `inspector` | `inspector` | `InspectorCounters`, a counting `TreeObserver` over the ADR-0040 observation seam: mounts, moves, rebuilds per cause, unmounts. |

All three features are on by default. A release build stays at zero devtools
cost by not depending on this crate, not by a feature flag here.

## Profiling a running app

The framework emits a `frame` span from `UpdateScheduler::drive_frame` and
`build`, `layout`, `paint`, `compositing` spans from the pipeline.
`FrameTimingLayer` subscribes to them; it never needs to be called.

```rust
use std::sync::Arc;

use flui_devtools::{FrameTimingLayer, Profiler};
use flui_log::{InstallPolicy, LogBridgePolicy, LogConfig};
use tracing_subscriber::layer::SubscriberExt;

let profiler = Arc::new(Profiler::new());
let subscriber = LogConfig::default()
    .subscriber()?
    .with(FrameTimingLayer::new(Arc::clone(&profiler)));
flui_log::install_subscriber(subscriber, InstallPolicy::Auto, LogBridgePolicy::Auto)?;

// run the app; `flui-app` inherits an installed subscriber
```

The layer carries its own per-layer filter that admits exactly the five spans
above. FLUI's default `INFO` log filter therefore does not starve it, and
attaching it does not turn on `DEBUG` logging for the whole process.

```rust
if let Some(frame) = profiler.frame_stats() {
    println!("{:.2} ms, jank: {}", frame.total_time_ms(), frame.is_jank());
    for phase in &frame.phases {
        println!("  {}: {:.2} ms", phase.phase.name(), phase.duration_ms());
    }
}
println!("avg {:.1} FPS, {:.1}% jank", profiler.average_fps(), profiler.jank_percentage());
```

`Profiler::with_config(ProfilerConfig { jank_threshold_ms, max_frame_history })`
sets the jank line (one 60 Hz frame budget by default) and the history depth.

The contract between the two halves is a set of span names, pinned by
`tests/frame_profile_end_to_end.rs`: it drives a real tree through
`HeadlessBinding::pump_frame`, the same `drive_frame` every runner uses, and
reads the profile back.

## Timeline

```rust
use flui_devtools::timeline::{EventCategory, Timeline};

let timeline = Timeline::new();
{
    let _guard = timeline.record_event("load assets", EventCategory::Custom);
    // ...
}
std::fs::write("trace.json", timeline.export_chrome_trace())?;
```

`Timeline::record_frame_snapshots` converts `flui_scheduler::FrameSnapshot`s
(from `FrameClock::frames_since`) into `Frame` events in the same trace, so a
presentation's frame history and hand-recorded events share one file.

## Inspector counters

```rust
use std::sync::Arc;

use flui_devtools::inspector::InspectorCounters;
use flui_foundation::observe::TreeObserver;

let counters = Arc::new(InspectorCounters::new());
build_owner.set_tree_observer(Arc::clone(&counters) as Arc<dyn TreeObserver>);
// ...drive frames...
let snapshot = counters.snapshot();
println!("{} mounts, {} rebuilds, {} unmounts", snapshot.mounts, snapshot.rebuilds, snapshot.unmounts);
```

This is the event half of ADR-0040's dependency-inverted seam: structural
observations pushed by the core, with no access to the trees themselves.
`flui-testing` drives a real tree against it as the seam's proof.

## What this crate is not

There is no inspector UI, no DevTools server, no network monitor, no memory
profiler and no remote-debug protocol. Hot reload is two other places: the
runtime half is `flui-hot-reload`, linked by the app; the source watcher and
rebuild loop are the `flui` CLI.

## License

MIT OR Apache-2.0
