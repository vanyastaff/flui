# flui-devtools

Runtime developer tooling for FLUI: the half that runs inside the
application. Small, feature-gated modules, each an adapter over a seam the
framework already exposes. Nothing here walks a widget, element or render tree
itself, opens an endpoint, or watches files.

| Module | Feature | What it is |
|--------|---------|------------|
| `capture` + `profiler` | `profiling` | `Capture`, the one recorder of the framework's frame spans, and `Profiler`: per-frame build/layout/paint/compositing and animation timings, jank detection, frame rate and history. `Capture::layer` subscribes to the framework's own frame spans. |
| `timeline` | `timeline` | An event recorder with Chrome trace (`chrome://tracing`) and JSON export, plus a bridge that turns the scheduler's `FrameSnapshot`s into trace events. |
| `inspector` | `inspector` | `InspectorCounters`, a counting `TreeObserver` over the ADR-0040 observation seam: mounts, moves, rebuilds per cause, unmounts. |
| `gesture_inspector` | `inspector` | `GestureInspector`, a `GestureObserver` on one arena that reports each contest: members with their composition paths, claims, the winner, rejections with reasons, elapsed deadlines and the input sequence. A recording is inspected by replaying it with the inspector attached. |
| `gesture_overlay` | `inspector` | `GestureInspectorOverlay`, a debug overlay at the app root that draws each live contest beside its contact, fading it once settled; a `GestureOverlayController` toggles it without rebuilding the app, and no observer is registered while it is hidden or unmounted. |

The first three features are on by default. A release build stays at zero
devtools cost by not depending on this crate, not by a feature flag here.

## Profiling a running app

The framework emits a `frame` span from `UpdateScheduler::drive_frame` and
`build`, `layout`, `paint`, `compositing`, `semantics`, `commit` and `submit` spans from the
pipeline and the runtime. The
`profiling` feature also enables animation instrumentation through the SDK:
registry walks, source sampling and listener/future delivery become the
`Phase::AnimationTick`, `Phase::AnimationSampling` and `Phase::AnimationListeners`.
`Capture::layer()` subscribes to them; it never needs to be called. A `Profiler` reads the
capture.

```rust
use flui_devtools::{Capture, Profiler, ProfilerConfig};
use flui_log::config::LogConfig;
use flui_log::ownership::{InstallPolicy, LogBridgePolicy, install_subscriber};
use tracing_subscriber::layer::SubscriberExt;

let capture = Capture::new();
let subscriber = LogConfig::default().subscriber()?.with(capture.layer());
install_subscriber(subscriber, InstallPolicy::Auto, LogBridgePolicy::Auto)?;
let profiler = Profiler::over(&capture, ProfilerConfig::default());

// run the app; `flui-app` inherits an installed subscriber
```

The layer carries its own per-layer filter that admits the frame/phase spans
and animation controller span identities. It admits no events. FLUI's default
`INFO` log filter therefore does not starve it, and
attaching it does not turn on `DEBUG` logging for the whole process.

Animation timings are inclusive: tick includes sampling and listeners, so
adding the three is not a frame CPU total. Reentrant work inside the same
phase is measured once. A phase belongs to its nearest scheduler frame;
standalone controller ticks and nested frames from another runtime are not
blended into the owning frame. CPU spans do not measure GPU execution.
Scheduler frames are recognized by their producer target as well as their name.
Animation phases and frames end at their outermost span exit; retaining a span
clone in application code cannot extend their timing into another frame.

All of this reads one `Capture`: `Profiler::over(&capture, config)` and `Timeline::over(&capture)`
are views over the same spans. A frame belongs to the runtime that drove it (the `runtime` field
of its `frame` span) and a span inside it to the presentation whose segment it ran in, so two
runtimes rendering under one subscriber are two `Partition`s of the capture. A `Profiler` has one
stream of frames: it follows the first runtime that names itself (or the one
`ProfilerConfig::runtime` names), and the capture keeps the others. Which spans are phases is
`Phase::of`, the one `(target, name)` table, so an application span named `paint` is not one. The
profiler records framework phases only; time your own work on a timeline with
`Timeline::record_event`.

Transition events can be logged separately with `animation-diagnostics` on
`flui` or `flui-sdk`, or `diagnostics` on `flui-animation`, and a DEBUG filter
for `flui.animation.diagnostics`. Instrumentation is off without these features
or devtools `profiling`; an interested subscriber is still required.

```rust
use flui_devtools::Phase;

if let Some(frame) = profiler.frame_stats() {
    println!("{frame}"); // frame #N: 12.34 ms (jank), then one line per phase
    if let Some(layout) = frame.phase(Phase::Layout) {
        println!("layout took {:.2} ms", layout.duration_ms());
    }
}
if let Some(rate) = profiler.frame_rate() {
    println!("{rate:.1} frames per second, {:.1}% jank", profiler.jank_percentage());
}
```

`frame_rate` counts the frames the scheduler produced per second, from the starts of the
retained frames; it is not a second divided by how long one frame's work took. An idle app
produces no frames, so its rate stays at the cadence of its last frames rather than falling
to zero.

`ProfilerConfig { jank_line, max_frame_history, runtime }` sets the jank line, the history
depth and the runtime followed. The line is `JankLine::FramePeriod` by
default: a frame is jank when it outlasts the period of the display it was
produced for, which the scheduler's `frame` span reports as `period_us`
(8.3 ms on a 120 Hz display; 16.7 ms when a frame reports none). `JankLine::Fixed(duration)`
keeps one threshold for every display.

The contract between the two halves is a set of span names, pinned by
`tests/frame_profile_end_to_end.rs`: it drives a real tree through
a headless UI runtime's frames, the same `drive_frame` every runner uses, and
reads the profile back.

## Timeline

```rust
use flui_devtools::Timeline;

let timeline = Timeline::new();
{
    let _guard = timeline.record_event("load assets");
    // ...
}
std::fs::write("trace.json", timeline.export_chrome_trace())?;
```

Every `TimelineEvent` says what it is with its `kind`: a `TimelineEventKind::Frame`, a
presentation `Segment`, a `Phase(phase)` of a captured frame, or a `Custom` event the
application recorded. `export_chrome_trace` writes one complete (`X`) event per span with its
kind's label as `cat`, a process per runtime, a thread per thread, and, for a timeline over a
capture, `frame_ms` and `jank` counters; it opens in `ui.perfetto.dev` and `chrome://tracing`.

`Timeline::record_frame_snapshots` converts `flui_sdk::hooks::FrameSnapshot`s
into `Frame` events in the same trace, so a presentation's frame history and
hand-recorded events share one file. The presentation's frame clock produces
the snapshots, and it is internal to the app host: no public API hands them
out yet (ADR-0088, move 4, names the follow-up).

## Inspector counters

```rust
use std::sync::Arc;

use flui_devtools::InspectorCounters;
use flui_sdk::foundation::observe::TreeObserver;

let counters = Arc::new(InspectorCounters::new());
build_owner.set_tree_observer(Arc::clone(&counters) as Arc<dyn TreeObserver>);
// ...drive frames...
let snapshot = counters.snapshot();
println!("{} mounts, {} rebuilds, {} unmounts", snapshot.mounts, snapshot.rebuilds, snapshot.unmounts);
```

This is the event half of ADR-0040's dependency-inverted seam: structural
observations pushed by the core, with no access to the trees themselves.
The seam's end-to-end test lives in this package
(`tests/tree_observer_inspector.rs`): it drives a real tree against the
counters.

Devtools is an official package: its only framework dependency is `flui-sdk`
([ADR-0088](../../docs/adr/ADR-0088-official-packages-sdk-and-facade.md)).

## What this crate is not

The one on-screen tool is the gesture overlay. There is no widget-tree
inspector UI, no network monitor, no memory profiler and no remote-debug
protocol. The driver agent, which serves a running app's windows to a driver, is
`flui-app`'s (ADR-0239). Hot reload is two other places: the
runtime half is `flui-hot-reload`, linked by the app; the source watcher and
rebuild loop are the `flui` CLI.

## License

MIT OR Apache-2.0
