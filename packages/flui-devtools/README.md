# flui-devtools

Runtime developer tooling for FLUI: the half that runs inside the
application. Small, feature-gated modules, each an adapter over a seam the
framework already exposes. Nothing here walks a widget, element or render tree
itself, opens a TCP port, or watches files.

| Module | Feature | What it is |
|--------|---------|------------|
| `profiler` + `frame_timing_layer` | `profiling` | Per-frame build/layout/paint/compositing timings, jank detection, FPS and history. Fed by a `tracing` layer that subscribes to the framework's own frame spans. |
| `timeline` | `timeline` | An event recorder with Chrome trace (`chrome://tracing`) and JSON export, plus a bridge that turns the scheduler's `FrameSnapshot`s into trace events. |
| `inspector` | `inspector` | `InspectorCounters`, a counting `TreeObserver` over the ADR-0040 observation seam: mounts, moves, rebuilds per cause, unmounts. |
| `agent` | `agent` | `AgentServer`, a `DevAgentHook` that serves the application's semantics tree to an agent over a local endpoint, in debug builds only. Off by default. |

The first three features are on by default. A release build stays at zero
devtools cost by not depending on this crate, not by a feature flag here.

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

`Timeline::record_frame_snapshots` converts `flui_sdk::hooks::FrameSnapshot`s
into `Frame` events in the same trace, so a presentation's frame history and
hand-recorded events share one file. The presentation's frame clock produces
the snapshots, and it is internal to the app host: no public API hands them
out yet (ADR-0088, move 4, names the follow-up).

## Inspector counters

```rust
use std::sync::Arc;

use flui_devtools::inspector::InspectorCounters;
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

## Serving the semantics agent

With the `agent` feature, `flui_devtools::agent::AgentServer` lets an agent (a
test driver, `flui mcp` once it exists) read a running app's semantics tree as
ADR-0080 wire nodes and act on its elements, through the same path assistive
technology takes ([ADR-0095](../../docs/adr/ADR-0095-agent-protocol-schema-crate.md) §3).

**Enabling it.** The application installs the server behind a development
feature of its own, so a release build never links it:

```toml
[features]
dev-agent = ["dep:flui-devtools"]

[dependencies]
flui-devtools = { version = "0.2.0-dev", default-features = false, features = ["agent"], optional = true }
```

```rust
let config = AppConfig::new();
#[cfg(feature = "dev-agent")]
let config = config.with_dev_agent(flui_devtools::agent::AgentServer::from_env());
run_app_with_config(App, config);
```

`AgentServer::from_env()` reads `FLUI_AGENT_ENDPOINT` (a pipe name on Windows,
a socket path elsewhere) and `FLUI_AGENT_TOKEN` (at least 32 bytes), which the
tool that launches the app sets. Without them, in a release build, or with a
short token the server stays inert and logs why once; if the endpoint cannot
be bound the app runs on without it. Either way the runner is told the server
does not serve, so no window collects semantics for it. The desktop and iOS runners hand the
server every window that mounts a root view; Android and web drive no agent.

**Framing.** Newline-delimited JSON, one request per line, at most 1 MiB. The
first line is `{"hello":{"token":"…"}}`, answered with
`{"hello":{"protocol":"0.1"}}`; then:

| Request | Reply |
|---|---|
| `{"id":1,"op":"windows"}` | `{"id":1,"result":{"windows":["w3"]}}` |
| `{"id":2,"op":"read","window":"w3","query":{"max_depth":4}}` | `{"id":2,"result":{"roots":[…]}}` |
| `{"id":3,"op":"act","window":"w3","request":{"element":"e7","action":"invoke"}}` | `{"id":3,"result":{}}` |

Element handles are scoped to their window, so every read and action names
one. A failure is ADR-0080's error object,
`{"id":n,"error":{"code","message","retry","kind"?,"handle"?,"effect"?}}`: a
closed window answers `gone` with kind `window`; a request the app does not
answer within the reply timeout (5 s) answers `timeout`, with retry `soon` for
a read and the `may_have_run` effect for an action, which is still queued and
runs at the next frame. Read the tree for an action's effect rather than
acting again.

**Security model.** The endpoint is local only: a named pipe that admits the
current user alone (by the process token's user SID, so an elevated app still
admits the same user's unelevated client) and refuses remote clients, or a Unix socket in a directory the
current user owns with mode `0700` (the server refuses any other). A client
must present the launch token first, compared in constant time; a wrong or
missing token closes the connection without a reply. The token keeps out
other users and remote callers, not other processes of the same user, which
can read the app's environment as they can reach the endpoint. What the
server logs carries operation names, window and element ids, error codes and
timings, never a label, a value or a request line.

**Try it.**

```sh
FLUI_AGENT_ENDPOINT=flui-agent-demo \
FLUI_AGENT_TOKEN=0123456789abcdef0123456789abcdef \
  cargo run -p flui-devtools --example agent_counter --features agent
```

The endpoint is pinned by `tests/agent_endpoint.rs`, which serves a headless
counter over a real pipe or socket.

## What this crate is not

There is no inspector UI, no network monitor, no memory profiler and no
remote-debug protocol beyond the agent endpoint above. Hot reload is two other places: the
runtime half is `flui-hot-reload`, linked by the app; the source watcher and
rebuild loop are the `flui` CLI.

## License

MIT OR Apache-2.0
