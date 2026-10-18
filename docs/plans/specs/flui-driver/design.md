# flui-driver: design (level 1)

- **Status:** draft, awaiting owner approval
- **Date:** 2026-10-17
- **Requirements:** [requirements.md](requirements.md). **Tasks:** [tasks.md](tasks.md)
- **ADRs:** [ADR-0238](../../../adr/ADR-0238-flui-driver-protocol.md),
  [ADR-0239](../../../adr/ADR-0239-every-debug-app-hosts-the-driver-agent.md),
  [ADR-0240](../../../adr/ADR-0240-semantics-identifier.md),
  [ADR-0241](../../../adr/ADR-0241-driver-trace-format.md)
- **Prior art read** (primary sources, 2026-10-17, as a checklist): Playwright (locators,
  actionability, web-first assertions, `toHaveScreenshot`, trace, wire protocol), W3C WebDriver BiDi
  (Editor's Draft 2026-10-08), Flutter `flutter_driver` and `integration_test`, Appium with
  UiAutomator2 and XCUITest, Apple XCUIAutomation, AccessKit and `accesskit_consumer`,
  kittest/egui_kittest, Playwright MCP, Windows-MCP, Slint's testing backend and in-app MCP server,
  Masonry `TestHarness`, GPUI `TestAppContext`, `iced_test`, and MCP revision 2026-07-28. Sources
  are cited inline where a decision leans on them.

## Shape in one picture

```text
 Rust test ─┐      flui drive (CLI) ─┐      flui mcp (stdio) ─┐
            ▼                        ▼                        ▼
   ┌─────────────────────── flui-driver ──────────────────────────────┐
   │ Driver ─ Target ─ Window ─ Locator/Element   (sync, typed)       │
   │ method table (flui-protocol) · waits · actionability · effects   │
   │ pixels (capture, baselines, predicates) · trace writer/replayer  │
   │ ┌──────────────┐ ┌──────────────────────────────────────────────┐│
   │ │ Connection   │ │ Lane: windows · macos · x11 · wayland ·      ││
   │ │ socket|bidi| │ │       android · ios_sim · chrome             ││
   │ │ in-process   │ │ (real input, OS a11y read, OS capture,       ││
   │ └──────┬───────┘ │  launch and process ownership)               ││
   └────────┼─────────┴──────────────────────────────────────────────┘┘
            │ NDJSON, method table subset "served by agent"
   ┌────────▼───────────── the app (debug, or `driver` feature) ──────┐
   │ flui-app agent endpoint ─► DevAgentHost ─► UiRuntime owner inbox │
   │ frame-boundary execution: locate · wait · read trees · inject    │
   │ synthetic input · actionability facts · capture presented frame  │
   │ · input journal and recording taps · events                      │
   └──────────────────────────────────────────────────────────────────┘
```

The app knows the truth: its trees, its frames, the input it actually received, the pixels it
presented. The driver composes that truth with what only the OS can do: deliver real input, read
what assistive technology sees, capture what the compositor shows, own the process. Every front
end calls the same driver methods.

## Options considered

| Option | For | Against | Verdict |
|---|---|---|---|
| **A. Grow `tools/desktop-mcp`** into every platform | Its safety rules and error model are proven on Windows | Black box only: no widget or render tree, no frame-timeline waits, no presented pixels. MCP-shaped core, so Rust tests would speak MCP. A tool crate, not publishable | Rejected (D1, D8) |
| **B. In-app only** (Flutter `integration_test`) | One process, deterministic, works on device farms | Never proves the OS delivers input. Flutter kept `flutter drive` for exactly that host-side role | Rejected (D1) |
| **C. Agent in the app plus OS lanes in one client crate** | The app answers what only it knows; lanes prove delivery; one protocol for the three front ends; headless is the same agent in-process | Two halves to version together; the app gains a local endpoint in debug builds | **Chosen** |
| **D. MCP inside the app** (Slint's `SLINT_MCP_PORT`) | Zero client code for agents | Every app carries an MCP SDK and an HTTP port; Rust tests would speak MCP; ADR-0095 already rejected it | Rejected |

Within C, the in-app server's home is the open question that ADR-0095 §3 had settled differently. That
section put the server in `flui-devtools`, installed only by app code. Then no example is drivable as
built (requirements R3), and Android and web drive no hook. ADR-0239 moves the server into
`flui-app`, active under `debug_assertions` or the `driver` feature, and makes every host drive it.

## Crates and placement

| Piece | Where | Why |
|---|---|---|
| Wire types and the method table | `flui-protocol` (tier C), schema `0.5` | The one contract the three front ends and the app share (ADR-0095 §1 stays: no upstream types, `serde`/`schemars` behind features) |
| Locator resolution, actionability facts, wait evaluation over semantics | `flui-semantics` (`agent` module, beside `read_wire`) | Pure functions over the published tree, so they are testable without a runtime |
| Element-tree inspection | `flui-view` (`inspect` module) | Only the element tree knows views, keys and element kinds; no public dump exists today |
| Render and layer inspection | `flui-rendering`, `flui-layer` (non-test `inspect` modules; the existing `testing::inspect` builds on them) | Same, for their trees |
| Owner-side agent ops (frame-boundary execution, input injection, journal, waits, events) | `flui-runtime` (`ui_runtime::agent`, extended) | It owns the owner inbox and the frame transaction |
| Endpoint server, transports, launch contract, presented-frame capture | `flui-app` (`app::agent`) | The composition root owns windows, the raster lane and the platform; ADR-0239 |
| Client: `Driver`, connections, lanes, pixels, trace, reports | **`flui-driver`**, new, tier H between `flui-app` and `flui-cli`, published, Evolving `0.N` | It depends on `flui-protocol` and, with feature `headless`, on `flui-testing`. It never depends on `flui-platform` or `flui-app` |
| CLI `flui drive …`, `flui mcp`, `flui inspect` | `flui-cli` | The `flui` binary is the user's entry point |
| MCP adapter | `flui-driver`, feature `mcp` (`rmcp`) | Reused by `flui mcp` and testable without the CLI |
| Inspector | `tools/flui-inspector`, an application on the facade and `flui-driver` | Dogfoods FLUI (D10) |
| Device scenarios | `tools/xtask/src/device/` (scenarios on `flui-driver`) | `cargo xtask device <check>` stays the entry point |

`flui-driver` holds OS code (`windows`, `objc2-*`, `x11rb`, `atspi`, `reis`, `tungstenite`). ADR-0082
keeps OS types inside `flui-platform` *for framework crates*. `flui-driver` is a client of the app,
never linked into it, and no framework crate depends on it. ADR-0238 records that the rule stays
enforceable: `cargo xtask workspace` gets `flui-driver` an `allowed-dependents` list of `flui-cli`,
the tools and dev-dependencies only.

## The protocol (ADR-0238)

### Envelope

NDJSON lines of at most 1 MiB on every byte transport. Binary payloads (captures) travel as
separate length-prefixed frames announced by the result (`"blob": {"id", "bytes", "media"}`), so a
PNG is never base64-inflated inside a JSON line. The BiDi transport carries blobs as base64 strings,
since BiDi has no binary frame.

```text
→ {"hello":{"token":"…","protocol":"0.5","client":"flui-driver/0.1.0"}}
← {"hello":{"protocol":"0.5","app":{…},"capabilities":{…}}}
→ {"id":7,"method":"semantics.locate","params":{…}}
← {"id":7,"result":{…}}            | {"id":7,"error":{"code":…,"message":…,"retry":…,"effect":…}}
← {"event":"frame","params":{…}}   (only after events.subscribe)
```

The shape is BiDi's (`id`/`method`/`params`; result or error; events by name), which the web lane
speaks anyway. It is not JSON-RPC 2.0: MCP's JSON-RPC lives only in the `flui mcp` adapter. Errors
keep ADR-0080's object, and ADR-0238 adds four codes:

- `ambiguous`: carries `candidates`, the first 10 outlines.
- `not_actionable`: carries `failed: attached | visible | stable | enabled | receives` and
  `covered_by`.
- `stale`: an element handle from an earlier generation that is still unknown after re-reading
  (distinct from `gone`, whose node the agent saw removed).
- `version`: client and app share no protocol version.

An older reader still reads every new code as `platform` (ADR-0095 versioning rule).

### The method table

```rust
// flui-protocol::method
pub trait Method: sealed::Sealed + 'static {
    /// "semantics.locate": the wire name, the CLI subcommand path and the MCP tool name ("semantics_locate").
    const NAME: &'static str;
    const SERVED_BY: ServedBy;      // Agent | Driver | Lane
    const EFFECT: MethodEffect;     // ReadOnly | Acts | Destructive  → MCP annotations
    type Params: Serialize + DeserializeOwned + JsonSchema;
    type Output: Serialize + DeserializeOwned + JsonSchema;
}
pub const METHODS: &[MethodInfo] = methods![ … ];   // one list; generated by the same macro
```

The trait is sealed, so the table is closed. `METHODS` drives the MCP tool list (sorted, deterministic),
the CLI subcommand coverage test, the schema golden file and the agent's dispatch. A unit test
fails when a method lacks a CLI subcommand or an MCP tool (R28). `ServedBy::Agent` methods are
what the app's endpoint answers. `Driver` and `Lane` methods are composed in `flui-driver`, so the
app never sees `launch`, OS input or baseline comparison.

| Group | Methods (`ServedBy`) |
|---|---|
| session | `session.launch`, `session.attach`, `session.close`, `session.capabilities` (D); `windows.list`, `window.placement` (A) |
| semantics | `semantics.read`, `semantics.locate`, `semantics.act` (A); `os.tree`, `os.compare_projection` (L) |
| inspect | `inspect.widgets`, `inspect.render`, `inspect.layers` (A) |
| input | `input.perform` (D: chooses the path); `input.inject` (A, synthetic); `input.os` (L); `input.journal`, `input.release` (A) |
| gestures | `element.tap`, `element.double_tap`, `element.long_press`, `element.drag_to`, `element.fling`, `element.swipe`, `element.pinch`, `element.scroll_into_view`, `element.type_text`, `element.press_keys` (D: locate → actionability → `input.perform` → observe) |
| facts | `element.actionability` (A) |
| waits | `wait.until` (A for app conditions; D adds a wall-clock or virtual deadline); `wait.settled`, `wait.frames` (A) |
| events | `events.subscribe`, `events.unsubscribe` (A) |
| capture | `capture.presented`, `capture.rendered` (A); `capture.os` (L); `pixels.expect_baseline`, `pixels.check` (D) |
| record | `record.start`, `record.stop` (A for input and native layers; D for the trace) ; `trace.replay`, `trace.codegen` (D) |
| motion | `motion.set` (A, the existing ADR-0176 op renamed) |
| perf | `perf.start`, `perf.stop` (A: frame and journal samples over the span); `perf.expect` (D: budget assertion) |
| audit | `audit.a11y` (A: static rules over the published tree); `audit.alternatives` (D: performs each gesture's alternative and compares effects); `audit.layout` (A); `audit.screen` (D: layout + a11y, with checked and not-checked lists) |
| layout | `layout.facts`, `layout.snapshot` (A); `layout.expect`, `layout.expect_snapshot`, `capture.annotated` (D) |
| explore | `explore.run`, `explore.shrink` (D) |

The existing agent ops become `windows.list`, `semantics.read`, `semantics.act` and `motion.set`.
Pre-1.0 this rename is listed in `version::BREAKING` with ADR-0238.

### Core types (all `#[non_exhaustive]`, `serde` and `schemars` behind features)

```rust
pub struct Locator {
    pub window: Option<WindowId>,
    pub role: Option<Role>,
    pub name: Option<TextMatch>,
    pub value: Option<TextMatch>,
    pub description: Option<TextMatch>,
    pub identifier: Option<String>,          // exact; ADR-0240
    pub state: StateFilter,                  // checked, disabled, focused, expanded, selected: Option<…>
    pub within: Option<Box<Locator>>,        // ancestor scope (Playwright chaining, BiDi startNodes)
    pub has: Option<Box<Locator>>,           // descendant filter (Playwright `has`)
    pub pick: Pick,                          // Strict (default) | First | Last | Nth(u32)
}
pub enum TextMatch { Exact { text: String, ignore_case: bool }, Contains { … }, Prefix { … } }

pub struct ElementRef { pub window: WindowId, pub element: ElementId }   // ElementId is generational
pub enum Target { Element(ElementRef), Locator(Locator), Point { window: WindowId, at: LogicalPoint } }

pub enum InputPath { Semantic, Synthetic, Os }
pub struct InputPolicy { pub prefer: InputPath, pub require: bool }   // Default: Synthetic, not required
pub enum Snapshot { None, Delta, Full }                                // MCP default Delta, Rust default None
pub struct SnapshotDelta { pub from: TreeRevision, pub to: TreeRevision,
                           pub appeared: Vec<Node>, pub gone: Vec<ElementRef>, pub changed: Vec<NodeChange> }

pub struct ActionSequence { pub sources: Vec<InputSource> }   // BiDi performActions shape
pub enum InputSource {
    Pointer { id: String, device: PointerDevice /* mouse|touch|pen */, actions: Vec<PointerAction> },
    Wheel { id: String, actions: Vec<WheelAction> },
    Keys { id: String, actions: Vec<KeyAction> },             // key vocabulary = flui-platform-api's
    Text { text: String },                                    // committed text
    Ime { actions: Vec<ImeAction> },                          // set_composition / commit / cancel
    Trackpad { id: String, actions: Vec<TrackpadAction> },    // pinch, rotate, pan phases
}
// ticks: every source's n-th action shares tick n; PointerAction::Pause/KeyAction::Pause keep alignment

pub enum Effect { Ran, MayHaveRun, Partial { sent: u32, total: u32, unit: Unit }, Incidental }
pub struct ActOutcome { pub via: InputPath, pub effect: Effect, pub observed: Option<JournalSpan>,
                        pub snapshot: Option<SnapshotReading> }   // Delta or Full, as asked
pub struct PerfReport { pub frames: u64, pub frame_time: Distribution, pub dropped: u32, pub janky: u32,
                        pub refresh_period: Option<Duration>, pub input_to_present: Distribution,
                        pub worst: Vec<FrameSample> }              // phase times per worst frame
pub struct AuditFinding { pub rule: AuditRule, pub node: ElementRef, pub detail: String }
pub enum AuditRule { Unnamed, FocusOrder, TargetSize, NoAlternative, AlternativeDiffers }

pub enum Condition {
    Appears(Locator), Gone(Locator), Matches { locator: Locator, expect: Expectation },
    Settled, Frames(u32), Event(EventFilter), All(Vec<Condition>), Any(Vec<Condition>),
}
pub enum Expectation { Name(TextMatch), Value(TextMatch), State(StateFilter), Count(u32), Focused }
pub struct WaitOutcome { pub satisfied: bool, pub frames: u64, pub observation: Observation,
                         pub unsettled: Vec<UnsettledCause> }   // pending frame, transient callbacks, timers, images
pub struct InspectNode { pub id: InspectId, pub type_name: String, pub properties: Vec<Property>,
                         pub rect: Option<Rect>, pub links: Links, pub children: Vec<InspectNode>,
                         pub omitted_children: u32 }
pub struct CaptureMeta { pub source: CaptureSource, pub size: (u32, u32), pub scale: f64,
                         pub frame: Option<u64>, pub region: Option<Rect>, pub blob: BlobRef }
```

`Node` gains `identifier` (schema 0.5). `Tree` stays ADR-0080's. `outline` stays the agent-facing
snapshot (`- button "Increment" [ref=e12] …`); refs are `ElementRef`s written `w3/e12` when a
session spans windows.

## Inside the app (ADR-0239)

### Execution model

Every agent method is an owner-inbox job run at the next frame boundary, as `SemanticsAgent` does
today (`flui-runtime/src/ui_runtime/agent.rs`). The endpoint thread never touches owner state; it
parses, enqueues, waits a bounded time and answers. Consequences:

- **Consistency.** A locate, an actionability check and an injection planned in one request see
  one committed tree (`semantics.locate` answers with the tree revision). The driver's composed
  `element.tap` sends `element.actionability` and `input.inject` as one `batch` job when the path
  is synthetic, so no frame runs between check and injection.
- **Waits do not poll.** `wait.until` registers the condition with the owner. The owner evaluates
  it after each frame's semantics publish and after each input delivery, and answers when it holds or
  at the deadline. Live targets express the deadline in the runtime's `ClockSource`, so a test's
  motion override (`motion.set`) also slows timeouts consistently. Headless waits run on the
  manual clock. The driver advances it frame by frame (`frame_dt`, default 16.667 ms) up to
  `max_frames`, so the outcome is deterministic.
- **Unsettled causes** come from the runtime's own predicates (`needs_redraw`,
  `is_frame_scheduled`, `has_pending_work`, `next_wake`, transient callback count, pending image
  loads), the same signals `pump_until_settled` reads (kittest reports repaint causes; Flutter
  `waitUntilNoTransientCallbacks`).
- **Reentrancy (F5).** Jobs run outside any borrow of the agent table. The owner commits the job's
  answer slot before running user-visible effects (semantics action handlers, injected input
  dispatch), and a callback that closes the window makes the answer `gone` rather than tearing the
  job. Answers are moved out before the reply is sent.
- **Containment (F4).** A job runs under the runtime's existing panic containment; a panic answers
  `platform` with the first failure, and the next job runs. A panicking condition evaluator drops
  only its own wait.

### Synthetic input

`input.inject` turns an `ActionSequence` into `PlatformInput` and feeds it into the window's
host input path, the same entry native events take (`OwnerHost` dispatch in `flui-app`; the
`HeadlessHost::dispatch` in tests). Recognizers, the held-input lane, input taps and recordings
therefore see it exactly as native input. Ticks map to frame boundaries: one tick's events are
dispatched together; a `pause` spans frames by the clock. Devices are registered with the platform
input vocabulary's device ids under a driver-owned device class, so a recording marks them. The
agent tracks every contact and key it pressed and releases or cancels them when the connection
closes or `input.release` is called (F2, R14).

### Input journal

A bounded ring per window (default 4096 entries) of delivered input, written by an input tap
(ADR-0215 `InputTap`) the agent installs. Each entry holds sequence, device class, kind, logical
position, timestamp and whether the driver injected it. `input.journal{since}` and `input` events
read it. OS-path actions observe through it (R13): after `input.os`, the driver waits up to the
action timeout for journal entries matching kind, device and position (within one device pixel
after the mapping in "Coordinates"). Match is `ran`; no match is `may_have_run` with what was seen.
Gesture-arena `Decision` rows from the same tap feed the inspector timeline.

### Trees

- `semantics.read` / `semantics.locate`: `flui-semantics::agent` resolves a `Locator` over the
  published `SemanticsOwner` tree. Strictness and `pick` are applied there, and it answers matches
  with their tree revision. `TextMatch` is exact/contains/prefix only, so no regex engine enters the app.
- `inspect.widgets`: new `flui_view::inspect::inspect_elements(root, InspectQuery)` walks the
  element tree. It reports the view type name (recorded at mount from `std::any::type_name`), key,
  element kind (`debug_kind`), lifecycle state, the linked render id, and `Diagnosticable` properties
  when the view provides them.
- `inspect.render`: from `PipelineOwner::debug_diagnostics_tree`, structured instead of rendered
  to text. It adds constraints, size, offset, dirty flags and the linked semantics node.
- `inspect.layers`: from `flui_layer`'s inspection, moved out of `testing` into a non-test `inspect`
  module that `testing::inspect` keeps building on.

Ids are `InspectId`s with a kind prefix (`v`, `r`, `l`) and the generational slot of the tree's
own ID (`ElementId`, `RenderId`, `LayerId`), so identity follows the "Identity is not a label" rule.

### Actionability facts

`element.actionability{element, point?}` answers the following, all in one frame:

- `attached`;
- `rect` (surface logical);
- `visible`: non-empty and intersecting the window's visible region and every clipping ancestor;
- `stable`: rect equal to the previous frame's;
- `enabled`: not `disabled`, no disabled ancestor;
- `receives`: whether the render hit-test at `point` (default: the rect's centre, clamped inside
  the visible part) resolves to a semantics node equal to or under the target. When it does not,
  the answer is `covered_by` (that node's outline).

The hit-test reuses `PipelineOwner`'s pointer hit-test and maps the topmost hit render object to
its nearest semantics owner. This mapping is new in `flui-semantics`, with the owner keeping a
render-to-semantics index already built at publish time. The driver polls `stable` across two
frames and repeats until the action timeout (Playwright's actionability, XCUI `isHittable`, Masonry's
interactive check).

### Presented-frame capture

`capture.presented{window, region?}` arms a one-shot request in the window's raster lane. The
next present copies the surface texture into a staging buffer before `present()`. The engine
already configures surfaces with `COPY_SRC` when the adapter allows (`renderer.rs:688`). The
buffer is read asynchronously off the frame path and answered with the frame id. When the
surface lacks `COPY_SRC`, the engine renders that frame's layer tree a second time into an
offscreen target of the surface's size and format, and the answer says `source: rendered`, never
`presented`. An occluded or minimized window that presents nothing answers `timeout` with the
reason. `capture.rendered` re-renders the last committed layer tree offscreen on request; it is
the headless source too (`HeadlessRenderer`). No capture blocks a frame: one pending request per
window, and a second one gets `busy`.

### Snapshot delta

Each acting job records the semantics revision it started on. When the action's effect has
settled (the composed method's own wait, or one frame for a bare act), the agent diffs the
published tree against that revision by `ElementId`:

- **appeared**: ids not in the old tree;
- **gone**: ids no longer in the tree;
- **changed**: same id, different name, value, description, state or actions. Bounds-only
  changes are left out unless asked, so a scroll does not flood the delta.

The diff is computed in `flui-semantics::agent` and capped like any read (`truncated`). MCP asks
for `Delta` by default; `Full` is the old outline.

### Performance samples

Between `perf.start` and `perf.stop` the agent keeps, per presented frame, its `FrameSnapshot`
(`flui-scheduler` `frame_telemetry.rs`):

- frame id, segment start and end, submit time, present outcome;
- the `input_epochs` it consumed, joined with the journal so each input gets the frame that first
  consumed it;
- the phase durations from the runtime's frame tracing.

`perf.stop` answers a `PerfReport`:

- frame-time percentiles (p50, p95, p99, max);
- dropped frames: a present slot missed against the presentation's refresh period, from ADR-0236's
  snapshot (unknown period answers `None`, and the budget then cannot judge jank and says so);
- janky frames: over one period of work;
- input-to-present latency: journal timestamp to present time of the consuming frame;
- the worst N frames with their phases.

`perf.expect` checks a `PerfBudget` (`max_dropped`, `p95_frame_time`, `p95_input_to_present` in
periods or milliseconds) keyed by target, for example `android:SM-G991B`. A target with no budget
entry is cannot-verify, never pass. Headless reports frame counts and phase work only, since
virtual time has no latency.

### Recording

`record.start{layers}` installs the ADR-0215 recording tap (input layer) and, where the build
carries `flui-platform`'s `native-capture`, the native capture hook (native layer). Rows stream to
the driver as `record` events. `record.stop` ends the recording with its `End` row. Privacy
follows ADR-0215 §5: text is redacted unless `keep_text` is set.

### Transports and launch contract

| Target | Endpoint | Credentials and options reach the app through |
|---|---|---|
| Windows | named pipe, current-user SID, remote refused (existing) | env `FLUI_AGENT_ENDPOINT`, `FLUI_AGENT_TOKEN` |
| macOS, Linux | Unix socket in a `0700` directory (existing) | env |
| iOS Simulator | Unix socket on the host filesystem (the simulator shares it) | `SIMCTL_CHILD_FLUI_AGENT_*` env on `simctl launch` |
| Android | abstract Unix socket `@flui-agent-<random>` (`interprocess` namespaced name); host side `adb forward tcp:0 localabstract:…` bound to 127.0.0.1 | intent extras `flui.agent.endpoint`, `flui.agent.token` on `am start`, read by the Android runner through the Java view (ADR-0237) |
| Chrome | BiDi: the driver adds a preload script that installs the token and a `channel` argument before the wasm starts. The web runner exports `__flui_agent.call(line) → Promise<string>`, and events go through the channel (`script.message`) | the preload script; no URL parameter carries the token |
| Headless | none: `flui-driver` calls the runtime's agent in-process | — |

`FLUI_AGENT_EXIT_ON_DISCONNECT=1` (env or extra) makes a launched app exit when its driver's
connection closes. Together with OS-level ownership (a Windows job object with kill-on-close,
Linux `PR_SET_PDEATHSIG`, a process group elsewhere, `am force-stop`, `simctl terminate`), this
ends a launched app when the driver dies (F2). An attached app never gets the flag.

**Builds.** The agent compiles under `cfg(any(debug_assertions, feature = "driver"))` in
`flui-app`; the facade's `driver` feature forwards to it. When the environment carries no
endpoint, the agent attaches nothing: no thread, no socket and no tap (R3). Release builds without
the feature contain none of the code.

## The client (`flui-driver`)

### API

```rust
use flui_driver::{Driver, Launch, Locator, Role, expect};

#[test]
fn increments() -> flui_driver::Result<()> {
    let mut driver = Driver::launch(Launch::from_env_or_headless(app))?; // FLUI_DRIVER_TARGET picks the target
    let window = driver.main_window()?;
    let button = window.locate(Locator::role(Role::Button).name("Increment"));
    button.tap()?;                                            // session InputPolicy decides the path
    expect(window.locate(Locator::identifier("counter"))).to_have_value("1")?;
    window.capture_presented()?.region_of(&button)?.expect_baseline("increment-pressed")?;
    Ok(())
}
```

- `Driver` owns one target and its connection. It is `Send` but not `Sync`; front ends wrap it in
  a worker when they need concurrency. Methods are synchronous. Lanes that need a thread affinity
  (UIA's COM apartment, AppKit's main thread) run on a lane worker thread the target owns, as
  desktop-mcp's `worker.rs` does today. A queued job is cancellable by one compare-and-swap.
- `Window`, `Element` and `ElementLocator` are cheap handles borrowing the `Driver`
  (`&mut Driver` for acts, `&Driver` for reads). Their methods mirror the method table one to one.
  Each gesture method has a builder for options (`.via(InputPath::Os)`, `.force()`,
  `.timeout(..)`).
- `expect(locator)` gives retrying assertions with a 5 s default (`to_have_value`, `to_have_name`,
  `to_be_checked`, `to_be_enabled`, `to_be_focused`, `to_have_count`, `to_be_gone`). Each is one
  `wait.until` with a `Matches` condition, never a client-side poll loop. `expect_soft`
  collects failures into the report.
- `Launch` describes the target and artifact (exe, `.app`, `.apk`, wasm bundle, or a headless view
  factory). Building is not the driver's business: `flui drive launch` and the xtask scenarios use
  `flui-cli`'s per-target build pipeline. `Launch::cargo_example` covers host-desktop tests.
- Errors: `flui_driver::Error`, a `thiserror` enum with a variant per ADR-0080/0238 code carrying
  its fields (`Ambiguous { candidates }`, `NotActionable { failed, covered_by }`, `Gone { kind }`,
  `NotSupported { capability }`…), plus `Io`, `Protocol` and `Lane`. `retry()`, `effect()` and
  `is_cannot_verify()` come from the code. The report's verdict rule reads `is_cannot_verify`.

### Connections

```rust
trait Connection: Send {
    fn call(&mut self, method: &'static str, params: Value, deadline: Instant) -> Result<Reply>;
    fn events(&mut self) -> &Receiver<Event>;
}
```

Three implementations: `SocketConnection` (NDJSON plus blob frames over `interprocess`, or over
the `adb`-forwarded TCP loopback), `BidiConnection` (`script.callFunction` and `script.message`)
and `InProcess` (headless, calling the runtime's agent directly, feature `headless`). The trait is
private: front ends see `Driver`.

### Coordinates (F9)

The agent answers surface-logical rects plus `window.placement`: the surface origin in screen
coordinates, the scale factor, and the monitor's device-pixel origin where the OS has one. Lanes
map them with checked arithmetic in `f64`, refuse non-finite results and refuse a point outside
the window's visible surface as `outside_target`. Points are never clamped into it. Mapping and
refusal sit in one pure module with a table test over extreme scales (0.5 to 8), negative monitor
origins, fractional boundaries and RTL mirroring (an RTL window's rects are already physical-left
based, so mapping is direction-free; a test pins that).

### OS input on a shared desktop

OS input is never the default (`InputPolicy::default()` is `Synthetic`). It runs only when the
session asks for it (`InputPolicy { prefer: Os, .. }`, `--via os`, or a scenario marked
`certification`). Three guards protect the person at the machine:

1. **Announcement.** Before the first OS event, the driver prints one line, for example "OS input:
   will take mouse and keyboard on this desktop for about 40 s; move the mouse to abort", to
   stderr and into the report and trace. A scenario declares its devices and estimated duration;
   an ad-hoc CLI call announces its own single action. On an interactive terminal on a shared
   desktop, `flui drive` also waits a 3 s grace period, which `--yes` skips.
2. **User-activity abort.** Each desktop lane watches for input it did not inject:
   - Windows: `WH_MOUSE_LL` / `WH_KEYBOARD_LL` hooks on the lane worker; events without
     `LLMHF_INJECTED`/`LLKHF_INJECTED` are the user's. Touch and pen come through
     `GetPointerInfo`'s source device, set against the synthetic device handle;
   - macOS: a listen-only `CGEventTap` comparing `kCGEventSourceUnixProcessID` and the source state
     with the lane's own source;
   - X11: XInput2 raw events from devices other than the XTEST virtual devices;
   - Wayland: none available, so OS input there requires an isolated session;
   - Android: on a physical device, `getevent` on input devices other than the lane's `uinput`
     device (`adb shell input` events come from no device node and are told apart by that); an
     emulator is isolated;
   - iOS Simulator and Chrome headless: isolated, no guard needed.

   The first foreign event makes the lane stop before its next event, `release_all`, and end the
   run *cannot verify* with `interrupted_by_user` (F11). A lane with no guard on a shared desktop
   refuses OS input with `not_supported{capability: "user_activity_guard"}` and names the
   isolation mode to use.
3. **Isolation modes.** `IsolationMode` is part of the session and the report:

| Platform | Isolation mode | How the driver uses it |
|---|---|---|
| Windows | Windows Sandbox (`.wsb` with the build mapped read-only, the driver launched inside) or a second user session | the driver runs inside and connects back over a mapped folder's named pipe relay, or a CLI run inside the session writes its report out |
| Linux | Xvfb (CI default) or a separate seat; for Wayland a headless or nested compositor (weston, cage) | `DISPLAY` / `WAYLAND_DISPLAY` set for the launched app and the lane only |
| macOS | a second user session (fast user switching) or a VM | the run happens in that session; the host session is untouched |
| Android | an emulator (`emulator -no-window`) when hardware fidelity is not the point | the target picks the AVD; a physical device is chosen explicitly |
| iOS | the Simulator | always isolated |
| Chrome | headless | always isolated |

`shared` is the mode of an ordinary desktop. It is allowed only with the announcement and the
guard.

### Lanes

```rust
trait Lane: Send {
    fn capabilities(&self) -> LaneCapabilities;
    fn inject(&mut self, seq: &ActionSequence, at: &Placement) -> Result<LaneEffect>;
    fn release_all(&mut self) -> Result<()>;
    fn capture(&mut self, window: &OsWindow, region: Option<DeviceRect>) -> Result<Image>;
    fn os_tree(&mut self, window: &OsWindow, query: &ReadQuery) -> Result<Tree>;
    fn launch(&mut self, launch: &Launch, agent: &AgentLaunch) -> Result<OsProcess>;
}
```

A trait object, because the lane is chosen at run time from the target. Each lane is a module
behind `cfg(target_os)` or a cargo feature (`android`, `ios-sim`, `chrome`, `wayland`; the host
desktop lane is default). An operation the lane lacks is `not_supported` naming it, before any
event is sent.

| Lane | Real input | OS accessibility read | OS capture | Process |
|---|---|---|---|---|
| windows | `SendInput` (mouse, keys, `VK_PACKET` text); `InjectSyntheticPointerInput` (touch, pen); TSF IME through the target's text store is out of the lane's reach, so IME uses `input.inject`. The safety checks are ported from desktop-mcp (`input/{mod,device}.rs`): foreground, window under each point, physical state, Sticky Keys, held-input recovery | UIA (`uiautomation`), the role table from `tools/xtask/src/device/uia_role_table.rs` | Windows Graphics Capture (`windows-capture`), window-scoped | `CreateProcess` in a kill-on-close job |
| macos | `CGEventPostToPid` (mouse, keys, scroll, tablet pen fields) via `objc2-core-graphics` | AX via `objc2-application-services` (`AXUIElement`), replacing `macos-ax-client.swift` | ScreenCaptureKit window capture (`objc2-screen-capture-kit`), replacing `screencapture -l` | `posix_spawn` in a process group |
| x11 | XTEST (`x11rb`), ported from `tools/live-smoke` | AT-SPI (`atspi`) | `GetImage` on the window | process group plus `PR_SET_PDEATHSIG` |
| wayland | libei through the RemoteDesktop portal (`reis`, `ashpd`); portal refusal is cannot-verify | AT-SPI | ScreenCast portal (`ashpd`, PipeWire), window-scoped | same as x11 |
| android | `adb shell input` (tap, swipe, keyevent, text, stylus, mouse) and `uinput` JSON-line scripts for stylus and touchpad, ported from `android_input.rs`. `uinput` without root works only on some devices, so it is a probed capability (`uinput_device`): the probe registers and removes a test device once per session, and stylus and touchpad answer `not_supported` where it fails; system UI such as the soft keyboard located by `uiautomator dump` | `uiautomator dump` for system UI only: FLUI publishes no Android accessibility tree, so FLUI nodes are `not_supported` until an adapter lands (recorded in `docs/gaps.md`) | `screencap -p` via `exec-out` | `adb install`, `am start` with extras, `am force-stop` |
| ios_sim | Simulator HID: `SimDeviceLegacyHIDClient` from `SimulatorKit.framework` through `objc2` dynamic loading, the channel `idb` uses. A runtime probe checks the selectors on first use and answers `not_supported` with the Xcode version when they are missing | none (FLUI's iOS backend publishes no AX tree; recorded in gaps) | `simctl io <udid> screenshot` | `simctl install/launch/terminate` |
| chrome | BiDi `input.performActions` (trusted input: mouse, pen, touch, wheel, keys); IME through `goog:cdp.sendCommand` `Input.imeSetComposition`/`insertText`, ported from `bidi.rs` | none: the canvas exposes no DOM accessibility tree for FLUI nodes today (recorded in gaps); `browsingContext.locateNodes` with `accessibility` reads it once a DOM mirror exists | `browsingContext.captureScreenshot` clipped to the canvas | ChromeDriver from Chrome for Testing (`chrome.rs`), page served by `tiny_http` |

### Determinism of OS lanes

Three rules come from what the interaction work measured.

- **Fixed window size.** A scenario declares its window (logical size, scale factor). The driver
  sets it at launch and refuses to run (`invalid_argument`, naming expected and actual) when the
  OS gives another, because HTML drops landed at 800×600 but not at 640×480.
- **Single-step moves.** A lane sends every `pointerMove` as one step to its destination,
  separated from the next action by an explicit `pause`. A gesture's path is a sequence of such
  steps generated by the driver, never a single multi-step move left to the OS or browser to
  interpolate. Chrome interpolates multi-step BiDi moves, which made recordings vary between runs.
- **Replay fidelity per lane.** `LaneCapabilities::fidelity` says what a lane reproduces exactly.
  It is recorded in the trace header (ADR-0241) and decides what replay and golden holds compare:

| Lane | Positions | Event order and kinds | Timing |
|---|---|---|---|
| windows | exact (device pixels) | exact | not compared |
| macos | exact (points) | exact | not compared |
| x11 | exact | exact | not compared |
| wayland | exact once measured; order-only until then | exact | not compared |
| chrome | exact with single-step moves | exact | not compared |
| android | order-only (the platform resamples moves) | exact | not compared |
| ios_sim | order-only until measured | exact | not compared |
| synthetic, headless | exact | exact | exact on the virtual clock |

A lane's row changes only with a measurement on that platform, recorded in the lane's module docs.

The iOS HID channel is a private framework. Using it is the owner's decision D5. The version probe
keeps a mismatch from crashing, and the first task is a spike with a go or no-go gate (tasks.md):
no-go escalates to the owner, never to a silent Swift fallback.

### Pixels

`pixels` compares RGBA8 images (`image` for codecs):

- **Stable capture.** Capture, then capture the next presented frame, until two consecutive
  captures are equal or the timeout passes (`toHaveScreenshot`).
- **Masks.** Each mask locator's rect is painted a fixed colour in both images before comparing.
- **Metric.** Per-pixel YIQ ΔE (the pixelmatch metric Playwright and kittest use), default
  threshold 0.1, plus `max_diff_pixels` (default 0) or `max_diff_ratio`. Anti-aliased pixels are
  not excused by default: FLUI's own rasterizer is the thing under test.
- **Baselines.** `<baseline dir>/<name>/<backend>-<os>.png`, backend from the capture
  (`presented-win32`, `rendered-headless`, …). The baseline dir defaults to `tests/baselines/`
  of the calling crate. A missing baseline fails and writes `.new.png`; a mismatch writes
  `.new.png` and `.diff.png`. `FLUI_DRIVER_UPDATE_BASELINES=failing|all` rewrites them, and is
  refused when `CI` is set.
- **Predicates.** `color_at(point, rgba, tolerance)`, `differs_from(&capture, region)`,
  `equals(&capture, region)`, `uniform(region, tolerance)`, `stable_for(frames)`.
- Fonts: captures come from apps on bundled fonts (`FontCollection::new()`), which
  `docs/testing.md` already requires of tests. A baseline is recorded per backend because GPU
  rasterization differs (kittest documents the sources: MSAA, filtering, float rounding).

### Trace (ADR-0241)

A trace is a directory `<name>.flui-trace/`:

- `trace.jsonl`: a header, then rows;
- `input.jsonl`: an ADR-0215 recording, verbatim;
- `native/<backend>.jsonl`: native recordings in the codec of
  `crates/flui-platform/tests/native_recording/`;
- `evidence/<sha256>.<png|json>`: content-addressed, so a repeated tree snapshot costs nothing.

```text
{"flui_trace":1,"protocol":"0.5","target":{"kind":"windows","backend":"win32","app":"interaction_gallery","profile":"debug"},"started_unix_ms":…,"privacy":"redact"}
{"command":{"seq":1,"t_ms":0,"method":"element.tap","params":{"target":{"locator":{…}},"via":"os"}}}
{"result":{"seq":1,"t_ms":41,"ok":{"via":"os","effect":"ran","observed":{…}},"evidence":["evidence/9f…json"]}}
{"event":{"t_ms":42,"name":"frame","params":{"frame":118}}}
{"gap":{"rows":3,"reason":"evidence_budget"}}
{"end":{}}
```

Commands carry locators as the caller wrote them, never only the resolved handle, so a replay
re-resolves (Playwright traces keep the locator). Evidence has a size budget per trace (default
256 MiB). Over budget, it writes a `gap` row and keeps going. A trace is complete only when it ends
with `end`, and a crash leaves an incomplete trace that readers report as such (ADR-0215's rule).
Modes: `off`, `on`, `retain_on_failure` (written to a temp dir, kept only when the report fails).

- `trace.replay(trace, target)` re-issues the commands and compares each result with the recorded
  one modulo volatile fields: handles renumbered by first appearance, times, frame ids. Evidence is
  compared through `pixels` with the trace's recorded tolerances. It answers a divergence report
  (first divergent seq, expected against actual).
- `trace.codegen(trace)` emits a Rust test using the API above: locators become `Locator`
  builders, waits become `expect`/`wait`, and captures become `expect_baseline` with the trace's
  evidence as the new baseline.

### Accessibility audit

`audit.a11y` runs in the agent over the published tree in one frame. Its rules are pure
functions in `flui-semantics::audit`:

- **unnamed:** an interactive role or a node with actions has no name and no labelled-by;
- **focus order:** traversal by the focus manager's order visits every focusable node once and
  follows reading order for the window's text direction, so a reversed pair in LTR or RTL is a
  finding;
- **target size:** an interactive node's hit area, its bounds grown to `GestureSettings::min_target`
  as the recognizers do, is below the minimum;
- **no alternative:** a node whose gesture recognizers (from the arena's registry) offer an
  interaction that no semantics action and no keyboard binding reaches.

`flui-testing`'s `assert_gesture_alternatives` and its minimum-target checks move onto these
functions, so the headless assertion and the driver share one implementation.
`audit.alternatives` is the dynamic half, composed in the driver. For each gesture-only
interaction it performs the gesture, records the snapshot delta, resets (relaunches or replays to
the start state), performs the alternative, and compares the deltas. When they differ, the finding
is `AlternativeDiffers`. The standard scenario `a11y-audit` runs both over every window of an app.

### Layout and design checks

The failure mode is common among design agents: they build a screen, take a screenshot, and judge
it with a vision model. The model does not notice a label 6 px off centre or two edges 2 px apart,
so the agent reports success. FLUI does not have to guess: the render tree knows every box, and
the text layout knows every line's ink bounds and baseline (`flui-painting` paragraph
`ink_bounds`, line metrics, `did_exceed_max_lines`). The driver checks facts and uses images only
as evidence.

**Facts the agent serves** (`layout.facts{nodes | locator}`, A). For each node, in one frame:

- the render box in window-logical coordinates, after transforms;
- the clip rect that applies;
- for text: ink bounds, line boxes, first and last baselines, the resolved `TextStyle`,
  `TextAlign`, whether it was ellipsized or exceeded its lines, and its painted colour;
- the composited background colour beneath the text, from the layer tree's opaque paints. When it
  cannot be determined (an image or gradient behind), it is sampled from a `rendered` capture and
  marked `sampled`;
- the parent's layout kind (row, column, stack, overlay, wrap) and alignment;
- the nearest semantics node and identifier.

These come from a new `inspect` query on `flui-rendering` (box, clip, transform), and from
`RenderParagraph`'s layout, exposed as data rather than diagnostics text.

**Assertions** (`layout.expect`, D) in the Rust API, CLI and MCP `layout` group:

```rust
expect(save_label).to_be_centered_in(&save_button, Axis::Both)?;                     // ink bounds vs content box
expect(&[name_field, email_field]).to_be_aligned(Edge::Leading)?;
expect(title).to_have_contrast_at_least(4.5)?;
expect(row.children()).to_have_equal_gaps()?;
expect(price).to_not_be_truncated()?;
```

The default tolerance is 0.5 logical px. A failure says what was measured: "ink centre (164.0,
22.5) vs container centre (158.0, 22.0): 6.0 px right, 0.5 px down".

**Lint** (`audit.layout`, A). The rules live in `flui-rendering::audit` (geometry) and
`flui-semantics::audit` (roles), as pure functions over facts:

| Rule | Fires when | Default threshold |
|---|---|---|
| `off_centre` | text whose `TextAlign` or parent alignment is centre has ink centre ≠ container content centre | > 0.5 px |
| `near_miss_alignment` | sibling leading or trailing edges, centres or baselines differ but by little | 0.5 < Δ ≤ 4 px |
| `uneven_gap` | in a row, column or wrap, one gap differs from the others while the rest are equal | > 0.5 px |
| `overlap` | sibling boxes intersect outside a stack, overlay or explicit z-order | area > 1 px² |
| `clipped_text` / `truncated_text` | ink bounds exceed the clip, or the paragraph ellipsized or exceeded `max_lines` unexpectedly | any |
| `outside_window` / `outside_safe_area` | content past the window or the safe-area insets | > 0 px |
| `low_contrast` | WCAG 2.2 relative-luminance contrast below 4.5, or 3 for large text (≥ 18.66 px, or ≥ 14 px bold) | ratio |
| `small_target` | shared with the accessibility audit | `min_target` |

The thresholds are configurable per app (`flui-driver.toml` or the session). A waiver names an
identifier, a rule and a reason. It is recorded in the report, so a waived finding is still visible
and never silent.

**Layout snapshot** (`layout.snapshot`, A; `layout.expect_snapshot`, D). It holds a JSON document
per window. For each identified node (and for role and name keyed nodes):

- rect, ink bounds, baseline;
- font family, size and weight;
- text colour and background colour, and whether the text was truncated.

Comparison uses geometric tolerances (default 0.5 px, colours by ΔE) and reports a numeric diff
per node. Baselines live beside pixel baselines (`<name>/<backend>-<os>.layout.json`); updates
follow the same explicit-update rule. This is the golden for design work: it survives the 1 px
glyph shifts that sank the pixel suite, and its diff says what moved.

**Annotated capture** (`capture.annotated`, D, `vision` and `layout` groups). It draws over a
presented or rendered capture: boxes, centre lines, baselines, gap measurements, and each
finding's rule and numbers, coloured by severity. It is meant for a person or a vision model to
look at. Findings never come from looking at it.

**Claim discipline.** `audit.screen` (core in MCP as `audit_screen`) runs `audit.layout` and
`audit.a11y` on a window. It answers the findings, an annotated capture's blob reference, and the
lists of `checked` and `not_checked` rules with reasons. `Report` carries the same two lists for
every check in a scenario. The verdict rule adds one clause: a check that was requested but did not
run makes the verdict at best `cannot_verify`, never `pass`. A design-agent workflow on FLUI is
then: build the screen, `wait_until` settled, `audit_screen`, fix the findings,
`layout_expect_snapshot` to pin the result. The agent can only claim success with an
`audit_screen` that has no unwaived findings.

### Parity coverage

Each scenario declares the parity rows it proves (`proves = ["pointer.touch", "ime.composition",
…]`), using the row keys of `docs/platform-parity.md`. Then:

- `flui drive report --parity` turns a passing report into the cells to update (environment, row,
  ✅, scenario, date, host);
- `cargo xtask parity` (a new gate in `checks`) parses the matrix and fails when a ✅ cell cites
  no scenario run, or cites a scenario that does not exist or does not declare that row. It also
  lists drivable rows with no scenario, as a warning until FD19 closes them and an error after;
- each `docs/gaps.md` entry names its closing scenario.

A green parity table is then a set of reports, not a set of claims.

### Exploration

`explore.run{window, seed, steps | budget, paths, invariants}` is composed in the driver from
public methods, so it works on every target:

- a `ChaCha8Rng` from the seed picks the next step among the actionable elements of the current
  snapshot, weighted by role: tap, long press, drag, scroll, text into text fields, key chords,
  back;
- the input path is the session's (synthetic by default);
- after each step the invariants are checked:
  - no contained frame failure or panic report;
  - no visible enabled element `not_actionable` other than `covered_by` another app node;
  - semantics consistent: unique ids, parent and child links agree, finite bounds;
  - no driver-injected contact or key still held.

The trace is always on. On a failure, `explore.shrink` delta-debugs the action list: it removes
chunks and replays from a fresh launch, keeps a shorter list only while it still fails the same
invariant, and stops at a 1-minimal list or a replay budget. The report names the seed, the
invariant and the minimal trace. Navigation away from the app (system back on the last route,
links) is excluded by a configurable `stay_within` predicate.

### Reports (R34)

```rust
pub struct Report { pub verdict: Verdict, pub checks: Vec<Check>, pub trace: Option<PathBuf>,
                    pub cannot_verify: Vec<Capability> }
pub enum Verdict { Pass, Fail, CannotVerify }
```

The verdict is `Fail` if any check failed, else `CannotVerify` if any requested check could not
run, else `Pass`. The report lists `checked` and `not_checked` (with reasons) for every check, so
a pass always says what it covered. `Report::exit_code()` maps them to 0, 1 and 2. A report prints the same way everywhere: a
human block plus one final line `FLUI_DRIVER_VERDICT=<pass|fail|cannot_verify>`, or `--json`. A
scenario is `fn(&mut Driver, &mut Report) -> Result<()>`. An `Error` that `is_cannot_verify`
becomes a cannot-verify check, never a failure.

## Front ends

- **Rust:** the API above; `flui-driver` with feature `headless` for in-process tests. The facade
  re-exports nothing of it; tests depend on it directly.
- **CLI:** `flui drive <group> <verb>`, mirroring the method table (`flui drive semantics locate
  --role button --name Increment`, `flui drive element tap …`, `flui drive capture presented -o
  shot.png`, `flui drive wait until --appears …`). Every method is also reachable as `flui drive call
  <method> --params <json>`, which keeps R28 true by construction, with ergonomic flags on top. A
  coverage test checks each method has its typed subcommand. `flui drive launch` writes
  `.flui/drive/session.json` (endpoint, token file path, target, pid). The token itself lives in a
  `0600` file beside it, never in the JSON or argv. Later commands read it (R30). Output follows
  flui-cli's policy (`--json`, `--quiet`, documented exit codes 0/1/2 for checks).
- **MCP:** `flui mcp` runs the `flui-driver` `mcp` adapter over stdio, `rmcp` at MCP 2026-07-28. Each
  method in `METHODS` has a `tier`: `Core`, or one of the capability groups. Tools are listed in
  `METHODS` order, name `group_verb`, input and output schemas from `schemars`, annotations from
  `EFFECT` (ADR-0080's widened error branch stays).
  - **Core (13, listed by default):** `session_launch`, `session_close`, `windows_list`,
    `semantics_read`, `semantics_locate`, `semantics_act`, `element_tap`, `element_type_text`,
    `element_press_keys`, `element_scroll_into_view`, `wait_until`, `capture_presented`, `audit_screen`. Acting
    tools answer a snapshot delta by default.
  - **Groups (`--caps a,b`):** `input` (`input_perform`, `input_inject`, `input_os`, the other
    gestures), `inspect`, `pixels`, `record` (with `trace_*`), `perf`, `audit`, `explore`, and
    `layout` (assertions, snapshots, annotated captures), `vision` (coordinate targets and `capture_os`, as Playwright MCP gates coordinate tools).

  Tool failure is `isError` with the error object; protocol failure is a JSON-RPC error. Captures
  return image content plus `CaptureMeta`. The adapter keeps no handle state: refs are `w3/e12`
  strings the app validates. A test checks that the core set stays at 13 tools or fewer and that
  every method belongs to exactly one tier.
- **Inspector:** `flui inspect` starts `tools/flui-inspector`. It is a FLUI app with material
  widgets, built on `flui-driver`, with tree panes for semantics, widgets, render and layers
  linked by the `links` ids; a live presented-frame view with click-to-pick (`element.actionability`
  at the point gives the hit node) and highlight; a properties pane; a timeline of frames, input
  journal entries and arena decisions from `events.subscribe`; and a trace player scrubbing
  `trace.jsonl` with its evidence. It has its own scenario suite driven by `flui-driver`.
  When a FLUI defect breaks the inspector, the CLI is the fallback that renders nothing with FLUI:
  `flui drive semantics read`, `inspect widgets|render|layers --json`, `capture … -o` and
  `trace show` cover everything the inspector shows except picking on a live image.
  `docs/testing.md` documents that path.

## Device checks become scenarios (R35)

| Today | Becomes |
|---|---|
| `windows-a11y` (UIA walk, invoke) | scenario: launch `a11y_probe`, `os.compare_projection`, semantic actions through UIA and through the agent, the roles page |
| `windows-gallery` (built on the integration branch now, against `examples/interaction_gallery`) | the **first scenario ported**: the same checks, by identifier, synthetic by default and OS input as the certification variant |
| `windows-input` (contract child test + SendInput probe) | scenario: `input.os` mouse, wheel, touch and pen against `interaction_gallery`, verified through the journal; native recording captured and held against the goldens (`record.start{native}`) |
| `windows-notes` | scenario on `two_screens`: chords, `VK_PACKET` text, IME through `input.inject`, DPI after resize |
| `web-input` (BiDi, wasm probe, JS API) | scenario on the chrome target: the same checks through the agent, and the probe's JS API is deleted |
| `android-input` (adb, uinput, F-key probe, `status.txt`) | scenario on the android target: lane input, journal verification, `record` events replace F12 flush, and `dumpsys` reads stay as lane helpers (`AndroidLane::shell`) |
| `macos-*` (Python, Swift, `screencapture`) | scenarios on the macos target; `tools/device-checks/` is deleted |
| `ios-*` (Swift XCUITest, Python) | scenarios on the ios_sim target |
| `live-smoke` (X11 XTEST) | scenarios on the x11 and wayland targets |

`plan.rs` keeps only build and package steps, and only until `flui-cli`'s build pipeline covers them.
The bespoke probes `android_input_probe` and `web_input_probe` are deleted, and so are their command
channels: F-keys, `status.txt`, `window.fluiProbe`. `interaction_gallery` replaces them as the shared
fixture, with an identifier on every interactive widget.

## Invariants, ownership, lifecycle

- The app owns identity. `ElementId`, `InspectId` and capture ids are minted by the app and
  generation-checked. The client and the MCP adapter hold strings and never mint. The semantic
  `ElementId` is the published node's generational `RenderId` (`flui-semantics/src/agent.rs:476`),
  so a recycled slot is a new identity. Generation exhaustion follows the owning arena, and a test
  pins that the agent refuses a handle it cannot tell apart, never re-binds it (F6). The same
  applies to the element and layer ids that `InspectId` wraps.
- One owner per window serializes jobs (F7). The endpoint keeps a bounded queue per connection
  (`busy` beyond it) and a connection cap. OS lanes take a host-wide input lock (a named mutex on
  Windows, a lock file elsewhere), so two drivers never interleave real input.
- Accepted work stays deliverable (F3). An action enqueued before its deadline stays queued; the
  reply says `may_have_run`. The driver never resends an `Acts` method on timeout; front ends
  surface the effect.
- Injected contacts and keys are owned by the connection and released on close (F2, R14). OS
  lanes release what they pressed on drop and on error, as desktop-mcp's `input_held` recovery does.
- The endpoint never runs user code. User code runs only inside owner jobs, after the job's answer
  slot is committed.

## Errors and failures

| Failure | Answer |
|---|---|
| app exits mid-call | `gone{kind: process, exit}`; every later call the same |
| locator matches 0 within the timeout | `not_found` (`retry: when_appears`) with the closest candidates by role and name |
| locator matches more than 1 under `Strict` | `ambiguous{candidates}` |
| actionability never holds | `not_actionable{failed, covered_by}` |
| OS refuses (foreground, covered, held input) | ADR-0080 codes with their data |
| lane missing or unusable | `not_supported{capability}`; the report's cannot-verify list |
| panic in a job | `platform` with the first failure; the agent keeps serving |
| version mismatch at hello | `version{app, client}` |
| capture over the size cap | `invalid_argument` naming the cap; region captures stay available |

## Test strategy

- **Contract tables.** In `flui-protocol`: schema golden `protocol-0.5.json`, containment of 0.4 in
  0.5, the `METHODS` coverage of the CLI and MCP, and wire spellings. In `flui-semantics`: one
  table of locator cases over a fixed tree (each criterion, scoping, strictness, `pick`, unicode case
  folding) and one of actionability (covered, clipped, offscreen, disabled ancestor, unstable).
- **Headless end to end** (`flui-driver`, feature `headless`, on `interaction_gallery`): every
  gesture through `synthetic` and `semantic`, waits on the virtual clock (deterministic frame
  counts), the failure matrix F1 to F10 (app panic, window closed by the action's callback,
  stale handle after slot reuse, exhausted generations, two clients, a timed-out act that runs at
  the next drain), trace record, replay and codegen round trip, and baselines on `rendered-headless`.
- **Agent over a real socket** on Linux CI: a debug example launched by `flui-driver`, driven over
  the socket, killed mid-call (F1). Then the driver is killed, and the test checks that the app
  exits and released its contacts (F2).
- **Lanes.** Each lane's pure mapping code (coordinates, key vocabulary to the OS keycode, BiDi and
  uinput serialization) is table-tested on every host. Each lane's live scenario runs on its
  platform. CI runs Linux x11 under Xvfb and Chrome headless; the rest run on the owner's machine
  or devices, and the PR says which ran (AGENTS.md platform rule).
- **Guards and determinism.**
  - User-activity guard: per lane, a foreign event injected through the OS's own test path (a
    second `SendInput` caller on W, a second XTEST client on X) aborts the run with
    `interrupted_by_user` and releases everything.
  - A lane with no guard refuses on a `shared` desktop.
  - A wrong window size refuses.
  - A multi-step move is split into single steps (a serialization table).
  - Replay compares positions only for exact lanes (a fidelity table).
- **Performance, audit, exploration.**
  - Perf: a headless table checks frame counts and phase work over a known animation. A live
    run on W and A asserts a scroll budget, and a deliberately slowed build (an injected busy
    frame) must fail it.
  - Audit: one rule table over fixtures with an unnamed button, a reversed focus pair in LTR and
    RTL, an undersized target, and a gesture-only control with and without its alternative.
  - Exploration: a fixture with a planted invariant break (a button that leaves a contact held)
    is found from a fixed seed, shrunk to the minimal trace, and reproduced by replay.
- **Layout and parity.**
  - Layout rules: one table over fixtures with a label off centre by 6 px, near-miss leading edges
    2 px apart, one uneven gap, sibling overlap in a column, an ellipsized title, content past the
    safe area, and contrast 4.4 vs 4.6 at the WCAG boundary. Each fixture also has a corrected
    twin, which must produce no finding.
  - Assertions report measured numbers.
  - Snapshot: a 1 px glyph shift within tolerance passes, a 6 px move fails with a numeric diff.
  - `audit.screen` with an unavailable rule answers it under `not_checked`, and the report is not
    `pass`.
  - `cargo xtask parity`: a self-test with planted cells (✅ with no scenario, a scenario not
    declaring the row) and a clean matrix.
- **Regression-proof.** Each fix and each new behaviour's test is run with the production hunk
  reverted in an isolated checkout and must fail for the intended reason (AGENTS.md).

## ADRs

- **ADR-0238.** The driver protocol and the `flui-driver` crate. It supersedes ADR-0080 in part: the
  desktop server is retired, the new error codes, `ElementRef` spelling, method envelope. It settles
  ADR-0095's still-Proposed items (finder criteria, handle kinds, effect type, `flui-mcp` becomes
  `flui mcp` over `flui-driver`, shared finder).
- **ADR-0239.** Every debug FLUI app hosts the driver agent; the server moves from `flui-devtools` to
  `flui-app`, every host drives it, there is a transport per platform, and the `driver` feature. It
  supersedes ADR-0095 §3.
- **ADR-0240.** A semantics identifier, exported through AccessKit `author_id`.
- **ADR-0241.** The driver trace format. It is related to ADR-0215, whose recording it embeds.

## Risks and open questions

- **SimulatorKit HID is private.** Mitigation: the version probe, the spike gate, and the
  `not_supported` answer. The owner chose it knowingly (D5).
- **Layout intent is inferred.** `off_centre` and `near_miss_alignment` infer intent from alignment
  and geometry, so a deliberate optical offset is a false positive. That is why waivers exist, and
  why findings carry their numbers and are reported, not silently fixed. The thresholds start
  strict and are tuned on the gallery and Notes before the lint becomes a default check.
- **A user-activity guard can miss input.** Low-level hooks see what the OS routes through them;
  a remote-desktop client or an accessibility switch device may not. The isolation modes are the
  real protection, and the announcement says so.
- **Windows Sandbox needs Windows Pro/Enterprise and virtualization.** Without it, the second user
  session is the Windows isolation mode; both are documented, and neither is required for
  synthetic runs.
- **Wayland portals ask the user.** RemoteDesktop and ScreenCast need consent per session (a
  restore token reduces it). Headless Wayland CI (weston) has no portal, so x11 stays the CI lane.
- **Presented-frame copy costs a copy per armed frame.** It is one-shot and never runs unrequested.
  The fallback is labelled `rendered`.
- **Android, iOS and web have no OS accessibility tree for FLUI nodes.** R17's cross-check runs on
  desktop only, and the gaps entries say so. The selector still works there through the agent.
- **Tier H publishing.** `flui-driver` is the first published tier-H crate besides the facade. If
  `cargo xtask workspace` refuses an Evolving kind in tier H, the task amends ADR-0081's kind table
  in ADR-0238 rather than bending the crate.
