# ADR-0403: A protocol method is named for the subject it addresses or the instrument it works, and one name serves the wire, the kit, the CLI and MCP

- **Status:** Accepted (2026-11-24)
- **Date:** 2026-11-24
- **Supersedes:**
  [ADR-0238](ADR-0238-flui-driver-protocol.md) §1's form of a wire name (`group.verb`), its list
  of a row's columns and its clause on what the MCP tool list is made from, only (§1, §2, §7
  here); §9's tier names and core set, only (§7; the bound of 13 stands); and the method names
  §10 and its Consequences spell (§3).
  [ADR-0288](ADR-0288-the-development-agent-is-one-sans-io-service.md) §8's clause that an
  observer never claims a lease, §9's `InputLease` as a lease of input alone, and §11's sentence
  on how many recordings a runtime and a connection hold, only (§1); and the names
  `input.release`, `record.start`, `record.stop` and the event names `input`, `record` and
  `record_native` of §9, §10 and §11, only (§3, §5).
  [ADR-0395](ADR-0395-a-rebuild-is-shown-over-the-app-it-will-replace.md) §1's sentence that
  `changes_app` is the one rule for what is sent again, §2's sentence that a report replaces the
  one before it whoever sent it, and §4's answer to a call that timed out, for a row that takes
  a hold, only (§1); and §2's name `build.report` (§3).
  [ADR-0401](ADR-0401-flui-run-serves-its-reload-at-an-endpoint-the-session-file-names.md) §1 and
  §4: the method name `run.reload` and the tool name `flui_reload`, only (§3, §7); §3's
  `run.reload` event of `flui run --json` keeps its name.
  [ADR-0400](ADR-0400-a-test-is-one-tester-over-a-connection-and-every-claim-has-one-instrument.md)
  §1's clause that `Placement` carries the platform convention (§6); §2's verbs `enter_text` and
  `scroll_until` and §5's `Tester::semantics` (§3); and the method name `audit.a11y` in §3 and
  §5 (§3). Its `settle`, `Settled` and `wait_settled` stand.
  [ADR-0095](ADR-0095-agent-protocol-schema-crate.md) §3's sentence that `flui mcp`'s tools are
  written in the CLI and map one to one onto the endpoint's operations, only (§7).
  [ADR-0176](ADR-0176-presentation-animation-playback.md)'s sentence that the operation is
  exposed as `motion`, and its silence on who restores a rate an agent set, only (§1, §3).
- **Related:** [ADR-0080](ADR-0080-agent-protocol-desktop-contract.md) (the error object);
  [ADR-0215](ADR-0215-input-recording-and-backend-conformance.md) (the input recording, and
  which text a recording keeps); [ADR-0239](ADR-0239-every-debug-app-hosts-the-driver-agent.md)
  (which builds serve the agent); [ADR-0241](ADR-0241-driver-trace-format.md) (a trace names
  methods); [ADR-0242](ADR-0242-typed-inspect-and-tracing-conventions.md) (the inspected node
  `inspect.inherited` answers);
  [ADR-0393](ADR-0393-a-development-reload-applies-one-of-three-tiers-chosen-from-the-shape-of-the-edit.md)
  (a restart composes the application again);
  [ADR-0402](ADR-0402-a-binary-links-the-development-agent-only-where-a-hook-is-installed.md)
  (how a new read reaches the runtime)
- **Sources:** `w3c/webdriver-bidi@cce53ea8cf` `index.bs`;
  `ChromeDevTools/devtools-protocol@856c4ed751` `json/browser_protocol.json`;
  `modelcontextprotocol/modelcontextprotocol@2bbace557b` `docs/specification/2026-07-28/`;
  `microsoft/playwright-mcp@2337aff021` `README.md`; `microsoft/playwright@b0e0b7b555`;
  `dart-lang/ai@04525f2972` `pkgs/dart_mcp_server/`;
  `ChromeDevTools/chrome-devtools-mcp@7fcebb2b03` `docs/tool-reference.md`;
  `appium/appium-xcuitest-driver@eb37b2a23e` and `appium/appium-uiautomator2-driver@f5daf7705d`
  `docs/reference/execute-methods.md`; `mobile-dev-inc/maestro@39361daf42`;
  `wix/Detox@667efcb6e7`; `flutter/flutter@ae9b586f34` (3.50.1). FLUI's own code is cited at
  `eb8afda901`.

## Context and problem statement

`flui-protocol`'s method table holds 46 methods in 16 groups
(`crates/flui-protocol/src/method.rs:475-615`), and the driver spec plans about 30 more. The
groups were named as each method arrived.

| Fact | Evidence |
|------|----------|
| The groups mix four kinds of word: a subject (`window`, `element`, `session`), a technique (`semantics`), a verb (`capture`, `record`, `inspect`) and the tool that serves (`run`) | the table |
| One subject has two spellings | `windows.list` beside `window.placement` |
| `motion` names the animation clock on the wire and the reduced-motion policy in the framework | `motion.set`; `MediaQueryData::motion`, `AppConfig::motion_override` |
| Two rows differ by a value the protocol already has a type for | `capture.presented`, `capture.rendered`; `CaptureSource` |
| The shipped MCP server lists seven hand-written tools named `flui_windows`, `flui_read`, `flui_locate`, `flui_act`, `flui_wait`, `flui_motion` and `flui_reload`; the table's own tool names are listed nowhere | `crates/flui-cli/src/agent/mcp.rs:148`; `MethodRow::tool_name` (`method.rs:388`) |
| That server answers `initialize` and `ping` and speaks the revisions 2025-06-18, 2025-03-26 and 2024-11-05 | `mcp.rs:51`, `:118` |
| It dials the app again inside a call when the connection broke, and says nothing of it | `Live::call`, `mcp.rs:458` |
| Its client drops every event that arrives during a call and queues the rest without a bound | `crates/flui-cli/src/agent/client.rs:176`, `:111` |
| A recording, a subscription and held input go with the connection that made them; the clock's rate and the build banner stay in the window whoever set them and whoever leaves | ADR-0288 §9, §11; `apply_motion` (`crates/flui-runtime/src/presentation/motion.rs:85`); `crates/flui-runtime/src/build_overlay.rs:61` |
| A runtime holds 8 recordings and a connection 4, with 4 connections admitted, so two observers can hold every one | ADR-0288 §11; `Limits` (`crates/flui-runtime/src/agent/service.rs:40`, `:43`) |
| The rows that start a recording, a span and a subscription are `ReadOnly` | the table |
| `session.launch` takes an executable and its arguments | `LaunchParams::artifact`, `::args` (`crates/flui-protocol/src/session.rs:286-298`) |
| Event names are bare words, and one of them, `window`, means opened or closed only | `crates/flui-protocol/src/events.rs` |
| Nothing reads the application's configuration, its build or a window's environment, though the window dispatch already reads the root media query | `crates/flui-runtime/src/ui_runtime/agent_serve.rs:403` |
| The current MCP revision has no session, forbids a tool list that varies by connection, and delivers server notifications only on a stream the client opened | Sources: `server/tools.mdx:65`, `basic/patterns/subscriptions.mdx:15` |
| Before the first release a wire name may be respelled once, in a version `BREAKING` lists | `crates/flui-protocol/src/version.rs` |

Scope: `flui-protocol`'s names, columns and conventions, who owns what a call leaves in the
app, the reads the protocol gains, and the two front ends that list the table, `flui drive` and
`flui mcp`. What a method does to the app is unchanged; who owns what it leaves, and when that
is released, is decided here (§1).

Which groups does the protocol have, how is a method placed and named, who owns what a call
leaves behind, and how does the table become a CLI and an MCP server?

## Decision drivers

- A method that arrives later has one group, found by a procedure two readers apply the same
  way, and no group is renamed for it.
- A name is the term its field uses, with a pinned source that shows it (AGENTS.md, "Code for
  the next reader"; `docs/adr/README.md`, "Sources are primary").
- One name on the wire, in the Rust API, in the CLI and in MCP (the driver spec's requirement
  that all front ends come from one table).
- The break is made once. No alias outlives it.
- **Nothing a development client sets outlives the client.** State a call leaves in the app has
  one owner and an exit, and a front end whose connection can end before its caller's next call
  either does not offer that state or says when it was lost.
- **A model can cause only what the person who started the server allowed.** The default tool
  list reaches one app and nothing else on the machine, and no text an app shows widens it.
- An event a client asked for is delivered or was never accepted: no stream without a reader,
  no queue without a bound.
- A model reads the tool list on every turn: a name says what it does without the others, the
  hints are true, and the list is small by default.
- A read reaches the runtime without linking the agent into a release binary (ADR-0402).
- A rule is held by a column, a type or a table test, not by a reader's memory (ADR-0303).

## Considered options

Groups:

1. Keep the groups and respell the few that clash.
2. Three subject groups only: `app`, `window`, `element`.
3. A closed table of groups, each of a stated kind (chosen).
4. Instrument groups only, as the DevTools protocol's domains.

Spelling on the wire:

5. `snake_case` (chosen).
6. `camelCase`.

What a call leaves in the app:

7. The window keeps what a client set: the clock's rate and the banner stay, as today.
8. The connection owns everything it leaves, through its lease on the window (chosen).
9. The driver's seat keeps its settings for a grace period after the driver goes.

The tool list:

10. One tool per method, a core set listed by default and the rest by tiers the person grants
    (chosen).
11. One tool per method, all listed.
12. An umbrella tool that takes the method's name.

The MCP revision:

13. 2026-07-28 alone.
14. The handshake revisions alone, as shipped.
15. Both: a request that carries the revision's metadata is served without a session, and
    `initialize` selects the handshake revisions (chosen).

Reads of the application's state:

16. Reads served from what the runtime and the service hold, at the scope that holds each
    (chosen).
17. App-level reads of whatever a test wants, `app.theme` among them, through a registry
    packages fill.

## Decision

Chosen: 3, 5, 8, 10, 15 and 16. Together they place every method by what its row says, keep one
spelling from the Rust type to the tool, give every piece of state a call leaves one owner that
ends with the caller, and keep a model inside what its person granted.

### §1. A row has six columns, a group has a kind, and a connection owns what it leaves

A row of the table states the method's name, who serves it, what it changes in the app, what it
leaves behind, how far it reaches, and where a front end lists it:

```rust
// flui_protocol::method
vocabulary! {
    /// What a call can leave in the app when it has answered: state the calling connection
    /// owns until it releases it or goes.
    pub enum Held {
        /// Contacts down and keys pressed.
        Input => "input",
        /// A recording of what a runtime applies.
        Recording => "recording",
        /// An event stream.
        Subscription => "subscription",
        /// A performance span.
        Span => "span",
        /// A rate set on a window's animation clock.
        Clock => "clock",
        /// The build status shown over a window.
        BuildStatus => "build_status",
    }
}

/// What a row does with a hold.
pub enum Hold {
    Nothing,
    Takes(Held),
    Releases(Held),
}

vocabulary! {
    /// How far a call reaches.
    pub enum Reach {
        /// The session's app, through its agent or its supervisor.
        App => "app",
        /// The machine: it starts a process, opens an endpoint the session did not have, or
        /// goes through the operating system.
        Host => "host",
    }
}

pub struct MethodRow {
    pub name: &'static str,
    pub served_by: ServedBy,
    pub effect: MethodEffect,
    pub hold: Hold,
    pub reach: Reach,
    pub tier: Tier,
}

impl MethodRow {
    /// Whether the call may be made again after a timeout on the same connection: a read that
    /// takes no hold, or an annotation, which the same lease shows again.
    pub const fn repeatable(&self) -> bool;
    /// Whether a call that reached a connection that then broke may be sent on another: a
    /// read that neither takes nor releases a hold. A hold went with the first connection.
    pub const fn resendable(&self) -> bool;
}
```

A row writes `holds:` and `reach:` only where they are not `Nothing` and `App`:

```text
Type = "group.member", ServedBy, MethodEffect, Tier, [holds: Takes(Kind),] [reach: Host,] Params => Output;
```

`ClientRole::may_call` still reads the effect alone: an observer calls what does not change the
app, a recording and a span among them, as `flui record` does today. What an observer may hold
is bounded below.

**Groups.** The groups are a closed table in `flui-protocol`, each with its kind. A group joins
the table in a record that names its kind and the source of its word.

```rust
pub enum GroupKind {
    /// Something a caller addresses. No row of it takes a hold.
    Subject,
    /// A development facility a call leaves state in. At least one row takes a hold, and every
    /// row that takes or releases that kind of hold is here.
    Instrument,
    /// A family of reads with one output type. Every row is read-only and takes no hold.
    Examination,
    /// The app as the operating system shows and drives it. Every row is a lane's and reaches
    /// the host.
    Lane,
}
pub const GROUPS: &[(&str, GroupKind)];
```

| Group | Kind | Is | Holds |
|-------|------|----|-------|
| `session` | subject | the client's connection to one target | `attach`, `capabilities`, `close` |
| `app` | subject | the application: its process, its build, its launch configuration | `launch`, `reload`, `build`, `config` |
| `window` | subject | one window | `list`, `placement`, `environment`, `snapshot`, `screenshot`, `announcements` |
| `element` | subject | one element, named by a target | `locate`, `actionability`, the gestures, `type_text`, `press_keys`, `perform_action` |
| `input` | instrument (`Held::Input`) | the input a client plays, and the journal of what a window was delivered | `inject`, `perform_actions`, `release_actions`, `journal` |
| `events` | instrument (`Held::Subscription`) | the connection's event streams | `subscribe`, `unsubscribe` |
| `clock` | instrument (`Held::Clock`) | a window's animation clock, the presentation's `MotionClock` (ADR-0176) | `set` |
| `overlay` | instrument (`Held::BuildStatus`) | what development tools draw over a window | `show_build_status` |
| `perf` | instrument (`Held::Span`) | frame performance over a span | `start`, `stop`, `expect` |
| `recording` | instrument (`Held::Recording`) | the input recording of ADR-0215: what a runtime applied, row by row | `start`, `stop` |
| `inspect` | examination (`InspectReading`) | the framework's own trees | `widget_tree`, `render_tree`, `layer_tree` |
| `audit` | examination (`AuditReport`) | rule checks that answer findings | `accessibility`, `screen` |
| `wait` | examination (`WaitOutcome`) | a request held until a condition answers | `until`, `settled`, `frames` |

Names reserved for groups that have no row yet. Each joins `GROUPS` with its first row:

| Group | Kind it will have | For |
|-------|-------------------|-----|
| `os`, `device` | lane | `os.snapshot`, `os.screenshot`, `os.perform_actions`, `os.compare_projection`; `device.list` and the device's lifecycle |
| `emulation` | instrument, with a `Held` of its own | overrides of what the platform reports, `emulation.set_preferences` first |
| `navigation`, `log`, `clipboard`, `storage`, `display`, `assets`, `permission`, `network` | stated by the record that lands the first row | a window's route stack; the app's log and failures; the device's clipboard, documents and displays |
| `layout`, `pixels`, `baseline`, `trace`, `explore`, `report` | stated by the record that lands the first row | the driver spec's facilities |

A contributor places a new method by these steps, taking the first that applies:

1. A lane serves it: `os`, or `device` for the device itself.
2. It takes or releases a hold: the instrument of that `Held` kind. A hold of a new kind is a
   new `Held` value and a new instrument group, added by a record.
3. Its output is an examination group's output type: that group.
4. It reads what an instrument holds or recorded: that instrument. A journal belongs to the
   instrument one of whose methods produces its entries (`input.inject` makes the entries
   `input.journal` reads), and to the subject where no method does (`window.announcements`:
   nothing speaks an announcement but the app).
5. It works one of the facilities the table of reserved names lists: that group, which the
   record that lands the row adds to `GROUPS` with its kind.
6. It is about the client's own connection: `session`.
7. Otherwise it reads or changes what a user would recognise as the app's, a window's or an
   element's: the narrowest of `element`, `window`, `app` that holds it.

A read of what the app sees lives with the subject whatever set it (`window.environment`
answers the value in effect, overridden or not); a read of what an instrument set lives with
the instrument. The clock has no separate read: `clock.set` with neither field answers its
state, as ADR-0176 decided.

**Ownership.** Everything a call leaves in the app belongs to the connection that made the
call, and goes when that connection does. Held input already works so (ADR-0288 §9), through
the connection's lease on the window; the lease now holds the rest of what a connection leaves
in a window:

```rust
// flui_view::dev_agent, in place of `InputLease` and `AgentWindow::input_lease`
/// One connection's hold on one window. `Send`, not `Clone`. Dropping it releases, at the
/// owner's next drain and before that drain reads its commands, everything the connection's
/// calls left in the window.
pub struct WindowLease { /* the window's port and the lease's id */ }
impl AgentWindow {
    pub fn lease(&self) -> Result<WindowLease, ErrorObject>;
}

// flui_runtime, one per lease, in the presentation
struct LeaseHolds {
    input: HeldInput,          // contacts and keys; a driver's lease only
    clock: Option<f64>,        // the rate the clock had before this lease set one
    span: Option<PerfSpan>,    // the lease's open span
    build_status: bool,        // whether the banner shown is this lease's
}
```

| `Held` | Taken by | Owner | Released by | Two connections |
|--------|----------|-------|-------------|-----------------|
| `input` | `input.inject`, `input.perform_actions` | the lease | `input.release_actions`; the lease's end | each lease releases only its own (ADR-0288 §9) |
| `clock` | `clock.set` naming a `rate` | the lease | the lease's end, which restores the rate the clock had; a step moves time and is not undone | one driver is admitted at a time, and only a driver may call it |
| `span` | `perf.start` | the lease, one span per window | `perf.stop`, which ends and reports the caller's span and answers `not_found` when it has none; the lease's end | a second `perf.start` on a lease starts its span again; another lease's span is not touched |
| `build_status` | `overlay.show_build_status` | the lease that reported last | a report of `succeeded`; the lease's end, which withdraws the banner if it is that lease's | the banner shows the last report of the lease that reported last |
| `recording` | `recording.start` | the conversation | `recording.stop`; the conversation's end (ADR-0288 §11) | a number is the connection's; another's is `unknown_handle` |
| `subscription` | `events.subscribe` | the conversation | `events.unsubscribe`; the conversation's end | as a recording |

- A session takes its lease on a window at its first call there that takes a hold, whatever its
  role. An observer's lease never holds input: its `input.*` calls are refused by its role
  before they reach the window.
- A span keeps what `PerfReport` answers, the distributions and the worst frames, never one
  sample for each frame it saw.
- A bound on a kind of hold keeps the driver's share. Of a runtime's 8 recordings, observers
  together hold at most 4, the bound less one connection's 4, so a driver's `recording.start`
  is never refused for what observers hold.
- A row that takes a hold is `repeatable` only when it annotates, and never `resendable`. A
  `recording.start`, `events.subscribe` or `perf.start` that timed out answers `may_have_run`,
  as a call that changes the app does, since a second one would start a second.

### §2. How a name is formed

```text
group.member          lower snake case; the group is one word with no underscore
```

- A subject group is singular. A group that is a stream or a medium is plural or a mass noun:
  `events`, `pixels`.
- An action's member is a verb, with its object where the verb alone is ambiguous
  (`perform_action`, `show_build_status`). An examination's member is the kind of examination
  (`accessibility`, `widget_tree`, `until`).
- A read's member is what it returns (`placement`, `snapshot`, `journal`, `build`). The read of
  a collection of handles is `list`. The one read that is named by its verb is
  `element.locate`: what it returns is whatever the locator matches, and `locate` is the
  field's word for it.
- A span is `start` and `stop`; a stream is `subscribe` and `unsubscribe`.
- The term the field shares wins; where the field splits, FLUI keeps its own word when three
  ecosystems use it. A changed or new name is recorded with its pinned source.
- The marker type is the name in upper camel case (`ElementPerformAction`). The CLI path is
  `flui drive <group> <member>` with `_` spelled `-`. The MCP tool is `group_member`. A
  `Tester` verb follows §3.

### §3. The names

Protocol 0.33 respells 19 methods, folds `capture.rendered` into `window.screenshot`, and adds
three reads. `METHODS` after it, in its order:

```rust
/// What is running: the application's name and identifier, its version, the host's build and
/// target.
AppBuild = "app.build", Agent, ReadOnly, Control,
    $crate::session::Empty => $crate::session::AppDescription;
/// What the application was configured with when it was last composed.
AppConfig = "app.config", Agent, ReadOnly, Control,
    $crate::session::Empty => $crate::app::AppConfigReading;
/// Start an app with the agent attached.
AppLaunch = "app.launch", Driver, Acts, Launch, reach: Host,
    $crate::session::LaunchParams => $crate::session::Attached;
/// Rebuild the application and apply the build at a tier, at least the one asked for.
AppReload = "app.reload", Supervisor, Destructive, Core,
    $crate::reload::ReloadParams => $crate::reload::ReloadReport;
/// Accessibility rules over a window's published tree.
AuditAccessibility = "audit.accessibility", Agent(window), ReadOnly, Audit,
    $crate::audit::AuditParams => $crate::audit::AuditReport;
/// Every rule this build applies, saying what was checked and what was not.
AuditScreen = "audit.screen", Driver, ReadOnly, Core,
    $crate::audit::AuditParams => $crate::audit::AuditReport;
/// Change, step or read a window's animation clock (ADR-0176).
ClockSet = "clock.set", Agent(window), Acts, Control, holds: Takes(Clock),
    $crate::clock::ClockParams => $crate::clock::ClockState;
ElementActionability = "element.actionability", Agent(window), ReadOnly, Input, …;
ElementDoubleTap = "element.double_tap", Agent(window), Acts, Input, …;
ElementDragTo = "element.drag_to", Agent(window), Acts, Input, …;
ElementFling = "element.fling", Agent(window), Acts, Input, …;
/// The elements a locator matches now.
ElementLocate = "element.locate", Agent(window), ReadOnly, Core,
    $crate::locator::LocateParams => $crate::locator::Matches;
ElementLongPress = "element.long_press", Agent(window), Acts, Input, …;
/// One semantics action on one element.
ElementPerformAction = "element.perform_action", Agent(window), Acts, Core,
    $crate::act::ActParams => $crate::act::ActOutcome;
ElementPinch = "element.pinch", Agent(window), Acts, Input, …;
ElementPressKeys = "element.press_keys", Agent(window), Acts, Core, …;
ElementScrollIntoView = "element.scroll_into_view", Agent(window), Acts, Core, …;
/// Scroll a scrollable a viewport at a time until a locator resolves inside it.
ElementScrollUntilVisible = "element.scroll_until_visible", Agent(window), Acts, Input,
    $crate::input::ScrollUntilVisibleParams => $crate::act::ActOutcome;
ElementSwipe = "element.swipe", Agent(window), Acts, Input, …;
ElementTap = "element.tap", Agent(window), Acts, Core, …;
ElementTypeText = "element.type_text", Agent(window), Acts, Core, …;
EventsSubscribe = "events.subscribe", Agent, ReadOnly, Record, holds: Takes(Subscription), …;
EventsUnsubscribe = "events.unsubscribe", Agent, ReadOnly, Record, holds: Releases(Subscription), …;
InputInject = "input.inject", Agent(window), Acts, Input, holds: Takes(Input), …;
InputJournal = "input.journal", Agent(window), ReadOnly, Input, …;
/// Input by the path the policy chooses.
InputPerformActions = "input.perform_actions", Driver, Acts, Input, holds: Takes(Input),
    $crate::input::InputParams => $crate::act::ActOutcome;
/// Release every contact and key the connection's lease holds.
InputReleaseActions = "input.release_actions", Agent(window), Acts, Input, holds: Releases(Input),
    $crate::session::WindowParams => $crate::input::Released;
InspectLayerTree = "inspect.layer_tree", Agent(window), ReadOnly, Inspect, …;
InspectRenderTree = "inspect.render_tree", Agent(window), ReadOnly, Inspect, …;
InspectWidgetTree = "inspect.widget_tree", Agent(window), ReadOnly, Inspect, …;
/// Show a window where the application's next build is.
OverlayShowBuildStatus = "overlay.show_build_status", Agent(window), Annotates, Control,
    holds: Takes(BuildStatus),
    $crate::overlay::BuildStatusParams => $crate::session::Empty;
PerfExpect = "perf.expect", Driver, ReadOnly, Perf, …;
PerfStart = "perf.start", Agent(window), ReadOnly, Perf, holds: Takes(Span), …;
PerfStop = "perf.stop", Agent(window), ReadOnly, Perf, holds: Releases(Span), …;
RecordingStart = "recording.start", Agent, ReadOnly, Record, holds: Takes(Recording), …;
RecordingStop = "recording.stop", Agent, ReadOnly, Record, holds: Releases(Recording), …;
/// Connect to an app already running with the agent.
SessionAttach = "session.attach", Driver, Acts, Launch, reach: Host, …;
SessionCapabilities = "session.capabilities", Driver, ReadOnly, Control, …;
SessionClose = "session.close", Driver, Destructive, Core, …;
WaitFrames = "wait.frames", Agent(window), ReadOnly, Control, …;
WaitSettled = "wait.settled", Agent(window), ReadOnly, Control, …;
WaitUntil = "wait.until", Agent(window), ReadOnly, Core, …;
/// The announcements spoken since a sequence number.
WindowAnnouncements = "window.announcements", Agent(window), ReadOnly, Record,
    $crate::events::AnnouncementsParams => $crate::events::AnnouncementJournal;
/// What the runtime installs above the window's root: the preferences in effect and the
/// platform convention.
WindowEnvironment = "window.environment", Agent(window), ReadOnly, Control,
    $crate::session::WindowParams => $crate::window::Environment;
/// The app's windows.
WindowList = "window.list", Agent, ReadOnly, Core,
    $crate::session::Empty => $crate::window::Windows;
WindowPlacement = "window.placement", Agent(window), ReadOnly, Control, …;
/// A window's pixels: the frame it last presented, or its layer tree rendered again.
WindowScreenshot = "window.screenshot", Agent(window), ReadOnly, Core,
    $crate::window::ScreenshotParams => $crate::window::Screenshot;
/// A window's semantics tree, or the part of it under one element.
WindowSnapshot = "window.snapshot", Agent(window), ReadOnly, Core,
    $crate::tree::ReadParams => $crate::tree::Tree;
```

A row ending `…` keeps its parameters and output. Around the rows:

- **`window.screenshot`** takes `source: Option<ScreenshotSource>`, a type with the two sources
  an agent serves, `presented` and `rendered`. Absent, the window answers its presented frame,
  and a host that cannot copy its surface answers the rendered one; named, it answers that
  source or `not_supported`. The answer's `source` stays a `CaptureSource`, whose third value,
  `os`, is the lane's `os.screenshot` alone. No host answers either source yet.
- **`app.launch` starts a process and never ends one.** `app.reload` is the one method that
  replaces a running app's code or its process, and only the supervisor that started the app
  serves it. A driver that launched the app itself has no supervisor: `app.reload` answers
  `not_supported` there (ADR-0401 §1), and the app is started again by `session.close`, then
  `app.launch`.
- **Tiers.** `Tier::Session` becomes `Tier::Control`: what the target can do, its build and
  configuration, a window's placement and environment, the frame and settle waits, the clock
  and the build status. `Tier::Launch` is new and holds `app.launch` and `session.attach`. A
  tier is a capability a person grants (§7); its name is not a group's, and it need not hold a
  group's rows. `MAX_CORE` stays 13: the core set loses `session.launch` and gains
  `app.reload`.
- **Types.** A type renamed here publishes its definition under its new name:
  `ClockParams`, `ClockRequest`, `ClockState`, `BuildStatusParams`, `ScreenshotParams`,
  `Screenshot`, `Windows`, `EventName`, `ScrollUntilVisibleParams`. `Windows` is the output of
  `window.list`, so no two public items are `WindowList`; `app.build` answers the hello's
  `AppDescription`, so no output is named as its marker is.

`version::BREAKING` gains:

```rust
Break {
    version: ProtocolVersion::new(0, 33),
    adr: "ADR-0403",
    renamed: &[
        ("audit.a11y", "audit.accessibility"),
        ("build.report", "overlay.show_build_status"),
        ("capture.presented", "window.screenshot"),
        ("element.scroll_until", "element.scroll_until_visible"),
        ("input.perform", "input.perform_actions"),
        ("input.release", "input.release_actions"),
        ("inspect.layers", "inspect.layer_tree"),
        ("inspect.render", "inspect.render_tree"),
        ("inspect.widgets", "inspect.widget_tree"),
        ("motion.set", "clock.set"),
        ("record.start", "recording.start"),
        ("record.stop", "recording.stop"),
        ("run.reload", "app.reload"),
        ("semantics.act", "element.perform_action"),
        ("semantics.announcements", "window.announcements"),
        ("semantics.locate", "element.locate"),
        ("semantics.read", "window.snapshot"),
        ("session.launch", "app.launch"),
        ("windows.list", "window.list"),
    ],
    removed: &[
        // The method folded into `window.screenshot`, as the schema test spells a method.
        "capture.rendered",
        // Definitions published under the names their types had.
        "/$defs/MotionParams", "/$defs/MotionRequest", "/$defs/MotionState",
        "/$defs/BuildReportParams", "/$defs/CaptureParams", "/$defs/CaptureMeta",
        "/$defs/WindowList", "/$defs/EventKind", "/$defs/ScrollUntilParams",
        // The eight event names of §5, each a variant of `Event`.
        "/$defs/Event/oneOf/frame=frame", "/$defs/Event/oneOf/input=input",
        "/$defs/Event/oneOf/semantics=semantics", "/$defs/Event/oneOf/window=window",
        "/$defs/Event/oneOf/announcement=announcement", "/$defs/Event/oneOf/gap=gap",
        "/$defs/Event/oneOf/record=record", "/$defs/Event/oneOf/record_native=record_native",
        // Fields.
        "/$defs/EventFilter/properties/kind",
        "/$defs/WindowEvent/properties/opened",
        "/$defs/Matches/properties/truncated",
        "/$defs/Placement/properties/platform",
    ],
    required: &["/$defs/EventFilter/required/event"],
}
```

The entry is complete when `every_published_schema_is_additive_to_the_next` passes over the
0.33 golden with no other edit: a path above that the test spells otherwise takes the test's
spelling, and a definition the golden lost that is not above is a defect of the change, not a
line to add to the test.

What changed, and the term each new name takes:

| Was | Is | The field's term |
|-----|----|------------------|
| `audit.a11y` | `audit.accessibility` | `appium/appium-xcuitest-driver` `execute-methods.md:2444`, `mobile: performAccessibilityAudit`; the DevTools protocol's `Accessibility` domain (`browser_protocol.json:8`) |
| `build.report` | `overlay.show_build_status` | the DevTools protocol's `Overlay` domain (`browser_protocol.json:20479`); the "error overlay" ADR-0395 cites; `flui run --no-build-overlay` |
| `capture.presented`, `capture.rendered` | `window.screenshot` | BiDi `browsingContext.captureScreenshot` (`index.bs:3877`) and the DevTools protocol's `Page.captureScreenshot` with `fromSurface` (`browser_protocol.json:23742`, `:23769`), each on the window with the source as a parameter; Playwright `Page.screenshot` (`docs/src/api/class-page.md:4094`) |
| `element.scroll_until` | `element.scroll_until_visible` | Maestro `scrollUntilVisible` (`YamlFluentCommand.kt:142`); `flutter_test` (`controller.dart:2381`) and `flutter_driver` (`driver.dart:422`) |
| `input.perform`, `input.release` | `input.perform_actions`, `input.release_actions` | BiDi `input.performActions`, `input.releaseActions` (`index.bs:15047`, `:15220`) |
| `inspect.widgets`, `inspect.render`, `inspect.layers` | `inspect.widget_tree`, `inspect.render_tree`, `inspect.layer_tree` | `flutter_driver`'s `get_render_tree`, `get_layer_tree` (`common/render_tree.dart:19`, `common/layer_tree.dart:19`); the Dart MCP server's `get_widget_tree` (`lib/src/mixins/dtd.dart:2146`) |
| `motion.set` | `clock.set` | FLUI's `MotionClock` (ADR-0176), the clock the method moves; Compose's `MainTestClock` (<https://developer.android.com/reference/kotlin/androidx/compose/ui/test/MainTestClock>). The connection's frame clock (`Connection::now`, `VirtualClock`) is not on the wire |
| `record.start`, `record.stop` | `recording.start`, `recording.stop` | FLUI's own noun: ADR-0215's input recording, the output type `Recording`, the handle kind `recording`, `flui record`. Maestro's `startRecording` (`YamlFluentCommand.kt:144`) and Playwright MCP's `browser_start_recording` (`README.md:1494`) use the word for video, which FLUI's is not. A trace (ADR-0241) is the driver's file of a session and may hold a recording |
| `run.reload` | `app.reload` | the Dart MCP server's `hot_reload` (`README.md:214`); BiDi `browsingContext.reload` (`index.bs:5156`) |
| `semantics.act` | `element.perform_action` | Detox `performAccessibilityAction` (`docs/api/actions.md:29`); Compose `performSemanticsAction` (<https://developer.android.com/reference/kotlin/androidx/compose/ui/test/SemanticsNodeInteraction>) |
| `semantics.announcements` | `window.announcements` | none: a journal read named by what it returns |
| `semantics.locate` | `element.locate` | BiDi `browsingContext.locateNodes` (`index.bs:4544`); WebDriver's `/element` endpoint (<https://www.w3.org/TR/webdriver2/>) |
| `semantics.read` | `window.snapshot` | Playwright `ariaSnapshot` (`class-page.md:2351`), Playwright MCP `browser_snapshot` (`README.md:1155`), Chrome DevTools MCP `take_snapshot` (`tool-reference.md:43`) |
| `session.launch` | `app.launch` | Appium `mobile: launchApp` (`execute-methods.md:271`), Maestro `launchApp` (`YamlFluentCommand.kt:120`), Detox `device.launchApp` (`docs/api/device.md:78`), the Dart MCP server's `launch_app` (`README.md:212`) |
| `windows.list` | `window.list` | Appium `mobile: listWindows` (UiAutomator2 `execute-methods.md:280`); Chrome DevTools MCP `list_pages` (`tool-reference.md:18`) |

The 26 names that stay are the field's already (`tap`, `swipe`, `pinch`, `attach`, `subscribe`,
`actionability`) or FLUI's own where the field has none or splits (`placement`, `journal`,
`settled`, `until`).

**`Tester` verbs.** ADR-0400 §4 and §6 stand: `pump`, `pump_for`, `settle` and `freeze_at` move
a virtual clock, and `wait_settled` is the `wait.settled` method on any connection. A verb of
the kit is one of two kinds, and a table in the kit lists every verb with its kind:

- A **method verb** sends one method and answers its output. Its name is the method's member,
  or `group_member` where the member alone is not a verb or two groups have it (`wait_until`,
  `wait_settled`, `wait_frames`).
- A **kit verb** moves the clock, composes several methods or projects an answer (`pump`,
  `settle`, `expect`, `read`, `rect`, `find_all`, `drag`, `background`, `foreground`). Its name
  is no method's member.

| Was (`crates/flui-testing/src/tester.rs`) | Is | Kind |
|-----|----|------|
| `enter_text` | `type_text` | method verb, `element.type_text` |
| `semantics` | `snapshot` | method verb, `window.snapshot` |
| `inspect_widgets`, `inspect_render` | `inspect_widget_tree`, `inspect_render_tree` | method verbs |
| `scroll_until` (ADR-0400 §2; not built) | `scroll_until_visible` | method verb |
| `pump_and_settle` | `settle` | kit verb: ADR-0400's name for it |
| `background`, `foreground` | unchanged | kit verbs of the headless host; they take their methods' names when `app.background` and `app.activate` are rows |

Each `try_` twin follows.

### §4. Wire conventions

- **Spelling.** Every name on the wire is lower snake case: a group, a member, a field, an
  enumerated value, an error code; a method and an event add one dot. A field's wire name is
  its Rust name. A value's wire name is its variant in snake case or the name its `vocabulary!`
  row or `serde(rename)` gives it, which is how a dotted name and a name Rust cannot spell are
  written.
- **Units.** A field that holds a duration or a time as a number names its unit: `_ms` in a
  method's parameters and answers, `_us` in an event's timestamp, `_ns` in a recording. The
  rows of an input recording keep `t` and `arrival`, nanoseconds by ADR-0215's format. Geometry
  is logical pixels by type (`LogicalRect`, `LogicalPoint`, `LogicalSize`); a count of device
  pixels says so in its documentation.
- **Bounds.** A read whose answer grows with the application's state takes a `max_…` parameter
  with a stated default, and says what it left out in one of two ways: `truncated: bool` where
  it cannot count (`Tree`, `InspectReading`), or `omitted`, a count, where it can
  (`Diagnostics`, and `Matches`, whose count named `truncated` becomes `omitted`). A node keeps
  `omitted_children`. A read is narrowed by its parameters (`root`, a locator); there is no
  cursor.
- **Journals.** A bounded journal is read with `since` and `max_entries` and answers `entries`,
  oldest first, each with its `seq`, and `dropped`. The last `seq` is the next `since`.
- **Errors.** A code names the state that refused, never the method. A new method uses a code
  the list has before it adds one; a new code is added by the record that needs it.
  `not_supported` names the missing `capability`. This record adds no code and two values:
  `HandleKind::Connection`, and the field `ErrorObject::lost: Vec<Held>`, left out when empty,
  for the one error a front end makes when a connection that held state is gone (§7).
- **Versions.** The break is one version. An app that speaks it refuses an older client
  `version`, naming both (`ProtocolVersion::serves`). No server accepts an old name. A trace
  names the protocol it was recorded in; the trace reader, which checks none today, refuses one
  a break separates from its own with the same error, and the golden trace is recorded again.

### §5. Events are named as methods are, and each front end delivers them its own way

An event line stays `{"event": <name>, "params": {…}}`. The name is `group.name`, and the group
is the one whose read returns the state the event reports.

```rust
// flui_protocol::events
vocabulary! {
    pub enum EventName {
        /// A frame presented, or was skipped.
        WindowFrame => "window.frame",
        /// A window opened.
        WindowOpened => "window.opened",
        /// A window closed.
        WindowClosed => "window.closed",
        /// The tree `window.snapshot` reads was published at a new revision.
        WindowSnapshotChanged => "window.snapshot_changed",
        /// An announcement joined the journal `window.announcements` reads.
        WindowAnnouncementAdded => "window.announcement_added",
        /// An entry joined the journal `input.journal` reads.
        InputEntryAdded => "input.entry_added",
        /// The subscriber's queue overflowed. Sent unasked.
        EventsGap => "events.gap",
        /// One row of a recording. Sent unasked to the connection that started it.
        RecordingRow => "recording.row",
        /// One line of a recording's native layer. Sent as its rows are.
        RecordingNativeLine => "recording.native_line",
    }
}

pub struct EventFilter {
    pub event: EventName,
    pub window: Option<WindowId>,
}
```

- An occurrence is a subject and a past participle (`opened`, `snapshot_changed`), as BiDi's
  `browsingContext.contextCreated` (`index.bs:5798`).
- A journal's event is `<entry>_added`, as BiDi's `log.entryAdded` (`index.bs:14791`), and
  carries the entry its read returns, with the same `seq`.
- A read a client reads again on change has `<read>_changed`, which carries the revision and
  never the value.
- An item of a stream is the bare noun (`frame`, `row`, `gap`), as BiDi's `script.message`
  (`index.bs:13999`).

A stream is delivered by the front end's own medium, and a front end that has none for an
event does not accept the request for it:

| Front end | A subscription | A recording |
|-----------|----------------|-------------|
| A `Connection` in Rust | the connection hands each event to its caller | the caller reads the rows |
| `flui drive` | prints each line until it is interrupted | writes the rows to a file until it is interrupted, then stops the recording: what `flui record` does |
| `flui mcp` | not a tool; a resource's change reaches a client on the stream it opened (§7) | the server writes the rows to a file and answers its link (§7) |

A front end's reader hands each line to its consumer as it arrives or drops it and counts it.
It keeps no queue of events without a bound.

### §6. Reads of the application's state

Three reads join the table. Each is served from state that exists, at the scope that holds it.

```rust
// flui_protocol::window
/// What the runtime installs above a window's root.
#[non_exhaustive]
pub struct Environment {
    pub window: WindowId,
    pub platform: Option<Platform>,
    pub brightness: Brightness,
    pub high_contrast: bool,
    pub motion: MotionPolicy,
    pub text_sizing: TextSizing,
    pub font_weight_adjustment: i32,
    /// BCP 47 tags, preferred first, at most 32; none when the host observed none.
    pub locales: Option<Vec<String>>,
    /// Whether `locales` was cut.
    pub truncated: bool,
    pub regional: RegionalFormats,
    pub accent_color: Option<Color>,
    pub scroll_bars: ScrollBars,
    pub view_insets: LogicalInsets,
    pub system_gesture_insets: LogicalInsets,
}

/// The authority that sizes text, as the root media query holds it.
#[non_exhaustive]
pub enum TextSizing {
    /// Authored sizes are used as they are.
    Fixed,
    /// Every size is multiplied by one factor.
    Linear { factor: f64 },
    /// The platform answers each size by itself: there is no one factor.
    Exact,
}

// flui_protocol::session — the hello's description, which `app.build` answers
#[non_exhaustive]
pub struct AppDescription {
    pub name: String,                    // the application's identity, not a window's title
    pub pid: Option<u32>,
    pub target: String,
    pub id: Option<String>,              // the application identifier, where it declares one
    pub version: Option<String>,         // the application's, where it states one
    pub arch: Option<String>,
    pub debug_assertions: Option<bool>,  // of the crate that hosts the agent
    pub flui: Option<String>,            // FLUI's version; the service fills it
}

// flui_protocol::app
/// What the application was configured with when it was last composed.
#[non_exhaustive]
pub struct AppConfigReading {
    pub diagnostics_profile: DiagnosticsProfile,
    pub exit_policy: ExitPolicy,
    pub window: WindowDefaults,          // title, size, bounds, mode, resizable, frame
    pub frame_latency: FrameLatency,
    pub input_latency: InputLatency,
    pub pointer_resampling: PointerResampling,
    pub motion_override: MotionOverride,
    pub performance_overlay: bool,
    pub storage: StorageReading,
    pub installed: Vec<InstalledHook>,
}
pub struct StorageReading { pub dir: StorageDir, pub placement: bool }
pub enum StorageDir { None, Default, Custom }   // never the path
pub enum InstalledHook { DevReload, DevAgent, DevTreeObserver, FrameFailureHandler, InputTap,
                         ErrorViewBuilder, CloseRequestHandler }

// flui_runtime::agent
pub struct ServiceConfig {
    pub app: AppDescription,
    pub config: Option<AppConfigReading>,   // none where the host composed no `AppConfig`
    /* token, limits, on_driver_gone, records_native as today */
}
impl Service {
    /// Replace what `app.build` and `app.config` answer, whole. A host calls it each time it
    /// composes the application.
    pub fn describe(&self, app: AppDescription, config: Option<AppConfigReading>);
}
```

- **`window.environment`** is a window method, served in the agent's window dispatch from the
  presentation's root media query and its platform convention, the values `window.placement`
  reads its size from. It answers each value as the root holds it, whatever set it: the
  runtime keeps one merged value for each field and no record of which were overridden, so the
  read has no such field. `Placement` loses `platform`; the size, scale and safe area stay
  there. `text_sizing` needs a public read of the policy's kind on `flui_painting::TextSizing`,
  whose policy is private today (`crates/flui-painting/src/text_painter/sizing.rs:47`, `:67`).
- **`app.build`** answers the hello's `AppDescription`: one type and one source, the value every
  service is made with. `Service::new` fills `flui` with its own crate's version and cuts each
  string to 256 bytes, so every host that serves the agent, `flui-app`, the headless kit and a
  test's own service alike, answers a bounded description without a rule of its own.
  `flui-app` passes the application's identity as `name`, where it passes the window title
  today (`crates/flui-app/src/app/agent/served.rs:52`). `version` is absent until
  `AppIdentity` can carry one.
- **`app.config`** is answered where a host handed a reading and refused `not_supported`,
  naming `app.config`, where it handed none; the hello then does not list the method. Only a
  host that composed an `AppConfig` has one to hand: `flui-app`. The headless kit's and a
  test's services pass `None` and claim no configuration. The rules about what leaves the app
  are the reading's types, not a host's care: `StorageDir` has no path, `InstalledHook` has no
  payload, and `Service::describe` cuts the title to 256 bytes.
- **When the readings change.** They are replaced whole each time the application is composed:
  at launch, and at a Restart reload, which keeps the process and composes the application
  again from its entry (ADR-0393). Launch is the only composition built today. A value the app
  changes while it runs (`set_motion_override`, `set_performance_overlay`) is read where it
  lives, in `window.environment` and the overlay, and `app.config` keeps what was configured.
- **Text.** `title` and `name` are fields the trace writer already redacts by name
  (`TEXT_FIELDS`, `crates/flui-driver/src/trace/redact.rs:21`); each new string field is put on
  one side by `every_text_field_of_the_wire_is_redacted_or_named_as_kept`.
- **There is no `app.theme`.** A theme is a value a package publishes down a subtree, which the
  runtime cannot name and which differs by element. Its home is `inspect.inherited`, the
  inherited values in effect at an element as inspected nodes, which needs a walk `flui-view`
  does not expose and an `Inspect` description the theme types do not have.
- **Overrides are `emulation`'s, when it is built.** An override of what the platform reports
  will be a hold of its lease, released with it, and needs a layer of its own in the runtime:
  `UiRuntime::motion_override` is one cell the application also writes
  (`crates/flui-runtime/src/ui_runtime/mod.rs:188`), which cannot hold a client's value and
  restore the application's. That layer, and the read of what it set, are decided with
  `emulation`, not here.

A read added later is a window method served in `agent_serve`, or a value a host hands the
service. It adds no call from the runtime's own paths (ADR-0402 §1), and its row and type state
its scope, its bound and the event that says it changed.

### §7. `flui drive` and `flui mcp` list the table

**The revisions.** `flui mcp` serves MCP 2026-07-28 and the three handshake revisions it speaks
today. A request that carries the revision's metadata is served with no session. An
`initialize` request selects the handshake revision it negotiates for the life of the stdio
process, as the 2026-07-28 revision lets a server do (`basic/versioning.mdx:183`).
`server/discover` answers the four. A request that names any other revision is refused with the
revision's own error and the list. The tools, their names, schemas and hints are the same in
every revision; a client on a handshake revision has no `subscriptions/listen`, and the server
declares no resource subscription to it.

**The list.**

```rust
// flui_protocol::method
pub struct ToolHints {
    pub read_only: bool,
    pub destructive: bool,
    pub idempotent: bool,
    pub open_world: bool,
}

impl MethodRow {
    /// `group_member`: the wire name with its dot as `_`.
    pub fn tool_name(&self) -> String;
    pub const fn tool_hints(&self) -> ToolHints;
}

// the adapter
pub struct ToolSelection {
    pub tiers: Vec<Tier>,     // --caps
    pub role: ClientRole,     // --role
    pub only: Vec<String>,    // --tools
    pub except: Vec<String>,  // --exclude-tools
    pub keep_text: bool,      // --keep-text
}
impl ToolSelection {
    pub fn lists(&self, row: &MethodRow) -> bool;
}
```

| Hint | True when |
|------|-----------|
| `readOnlyHint` | the effect is `read_only` and the row neither takes nor releases a hold |
| `destructiveHint` | the effect is `destructive` |
| `idempotentHint` | the effect is `annotates` |
| `openWorldHint` | the reach is `host` |

- Every row is a tool unless its hold has no place in an MCP server (the table below). None is
  written by hand, and there is no umbrella tool.
- The list is the core tier, each tier `--caps` names, less what `--role observer` may not
  call, filtered by name, in `METHODS`' order. It is fixed when the server starts and is the
  same whichever app the server reaches; a tool the app does not answer fails `not_supported`.
- **Authority.** A row whose reach is `host` is in `Tier::Launch` or `Tier::Vision` and never
  in the core. So a server started with no flag drives the one app its session file names: it
  starts no process, opens no other endpoint and sends no input through the operating system.
  `--caps launch` lists `app_launch` and `session_attach`, which move the server to another
  app; their descriptions say so. `recording_start` refuses `keep_text` unless the server was
  started with `--keep-text`. What a snapshot quotes is the app's text and grants nothing.
- The core set is `app.reload`, `audit.screen`, `element.locate`, `element.perform_action`,
  `element.press_keys`, `element.scroll_into_view`, `element.tap`, `element.type_text`,
  `session.close`, `wait.until`, `window.list`, `window.screenshot` and `window.snapshot`.
  `session.close` ends only an app the session launched (`CloseParams::keep_app`,
  `crates/flui-protocol/src/session.rs:373`).

**Holds.** A front end's connection can be shorter than what a call leaves on it: `flui drive`
makes one for each command, and `flui mcp` dials again when the app restarts. Each offers a
hold only in a form it can keep or account for:

| `Held` | `flui drive` | `flui mcp` |
|--------|--------------|------------|
| `input` | a sequence that leaves a contact or a key held (`ActionSequence::leaves_held`, which the protocol gains) is refused `invalid_argument` before anything is sent; there is no `release-actions` command | the same; `input_release_actions` is not listed |
| `subscription` | the command prints events until it is interrupted | `events_subscribe` and `events_unsubscribe` are not listed |
| `build_status` | the command shows the status until it is interrupted | not listed: the status is its builder's to report, and a reload the server asks for is built and reported by the run |
| `recording` | the command records to a file until it is interrupted | listed and kept in the ledger; the server writes the file |
| `span` | the command holds its connection for `--for`, then stops and prints the report | listed and kept in the ledger |
| `clock` | a rate is set for as long as the command holds its connection; a step and a read answer at once | listed and kept in the ledger |

A row that releases a hold has no `flui drive` command: a new connection holds nothing to
release, and the command that took the hold releases it when it ends.

**The ledger.** `flui mcp` keeps, for its connection to the app, the set of `Held` kinds its
calls have taken there. When a call finds that connection gone:

- with an empty ledger, the server dials again and sends the call, as it does today; a call
  that had reached the old connection goes again only if its row is `resendable`;
- otherwise the server sends nothing. It answers that call `gone`, with `kind: connection` and
  `lost` listing the ledger, empties the ledger, and dials at the next call.

So a model learns once, before anything else happens, that the clock runs at the app's own
rate again, that its span is gone, and that its recording ended.

**Recordings.** `recording_start` makes the server the recording's reader: its connection
writes each `recording.row` to a file under the server's output directory as the row arrives,
and the answer carries the file as a resource link. The handle a model is given is the
server's own number, minted once in the server process and never used again, so a number from
before a redial cannot name a later recording. `recording_stop` answers the summary and the
link. A recording whose connection ended is closed with what arrived, and `recording_stop`
answers it once with `complete: false`; after that the number is `unknown_handle`. A file is
cut at the server's size bound, the recording stopped as if asked.

**Events.** A client on revision 2026-07-28 opens `subscriptions/listen` naming resource URIs.
For as long as that stream is open the server subscribes, on its connection to the app, to the
events `RESOURCES` gives for those URIs and no others, and sends
`notifications/resources/updated` for a URI when one arrives. It keeps one pending update for
each URI and stream, not the events. The server's own subscription follows the open streams:
it is made again after a redial and is never in the ledger. The revision delivers nothing else
on that stream (`basic/patterns/subscriptions.mdx:15`), so no other event reaches an MCP
client; a model that needs to know when something happened holds `wait_until` with an event
condition, and reads what happened since from a journal.

**Schemas and results.** `inputSchema` is the method's `Params`; `outputSchema` is its `Output`
or `{ "error": ErrorObject }`. The result's `structuredContent` conforms, and its text is the
serialized JSON, except that a snapshot's text is the outline, whose references are written
whole (`w3/e12`), and a screenshot is an image block.

**Targets.** A model reads `window_snapshot` and acts on a `Target`: a reference from the
outline or a locator. An act answers a snapshot delta by default (`Snapshot::Delta`,
`crates/flui-protocol/src/snapshot.rs`), which the app computes from the revision the act
started on. A reference the app no longer knows fails `stale` or `gone` and is never bound
again.

**Errors.** An `ErrorObject` is a result with `isError`. A parameter that does not read as the
method's `Params` is `invalid_argument`, returned the same way. A call with no answer whose row
is not `repeatable` is an error with `effect: may_have_run` and is not sent again. Only an
unknown tool or a malformed request is a JSON-RPC error.

**Resources and prompts.**

```rust
pub struct ResourceRow {
    pub method: &'static str,                // read-only, takes no hold, parameters a window or none
    pub uri: &'static str,                   // `flui://window/{window}/snapshot`
    pub updated_by: &'static [EventName],
    pub media_type: &'static str,
}
pub struct PromptRow {
    pub name: &'static str,
    pub tools: &'static [&'static str],
    pub text: &'static str,
}
```

A resource is such a method under `flui://<group>/<member>` or
`flui://window/{window}/<member>`, and is a tool as well. A prompt is a scenario a person
picks; each names the tools it uses.

**State.** The server keeps none for an MCP connection. Windows and elements are the app's
handles. The ledger, the recordings' files and the open streams' subscriptions are the server
process's, about its one connection to the app.

### Consequences

- Good, because a later method is placed by what its row says, and a test fails a row in the
  wrong kind of group.
- Good, because a model's tool list reads as the wire does, and the CLI, the kit and a trace
  spell a method one way.
- Good, because a client that leaves, by its own end or a broken socket, leaves nothing set:
  no paused clock, no banner, no held key, no span.
- Good, because a model's server reaches one app unless its person starts it otherwise, and a
  tool that starts a recording or a span is no longer hinted read-only.
- Good, because the reads of §6 cost the runtime no state and a release binary no code.
- Bad, because 19 names, 8 event names, 5 `Tester` verbs and a tier change at once: several
  hundred mentions across `crates/`, `docs/` and `changelog.d/`, most of them in
  `flui-protocol`, `flui-cli`, `flui-runtime` and `flui-driver`, the schema's next golden and
  the golden trace.
- Bad, because an app that speaks 0.33 refuses every older client, and a `flui` tool older than
  the app it finds says `version` and nothing more helpful.
- Bad, because a setting no longer survives between two `flui drive` commands: a paused clock
  is held by the command that paused it, for as long as that command runs.
- Bad, because a model cannot launch an app, attach to another, show a build status or hold a
  key across two calls unless its person granted the first two; the last two it cannot do.
- Bad, because the first call after a broken connection that held state fails, to say what was
  lost.
- Bad, because `flui mcp` carries two framings until the handshake revisions are dropped, which
  a later record decides.
- Bad, because `element.perform_action` and `input.perform_actions` differ by a letter and a
  group. They are in different tiers, and each description opens with the difference.
- Bad, because the theme, the resolved locale and the text direction stay unread until
  `inspect.inherited` is built, and an override until `emulation` is.
- Bad, because the test of the kit's verbs sees the verbs its table lists: a verb added to
  `Tester` and left out of the table is not seen by it.
- Neutral, because `flui run --json` keeps its own event names (`run.reload`, `build.phase`):
  that stream is the command's, not the protocol's.

### Confirmation

Three changes build this record: the names, events and conventions of §3 to §5 with protocol
0.33; the columns and the lease of §1 and the reads of §6, in that version or a later additive
one; and the front ends of §7, which need `flui drive` and a generated adapter that do not
exist yet. `docs/gaps.md` has one bullet for each. Until §7 is built the seven hand-written
tools keep their `flui_*` names and call the methods under their new names.

| Rule | Held by |
|------|---------|
| A name is `group.member` in lower snake case, a group has no underscore, the table is sorted, and a tool name splits back into its method | `flui-protocol`'s `every_method_is_named_group_dot_member_once_and_listed_in_order`, which today's `every_method_is_named_group_dot_verb_once_and_listed_in_order` becomes |
| Every group is in `GROUPS`, and every row fits its group's kind: a subject group takes no hold; an instrument group has a row that takes its kind, and every row that takes or releases that kind is in it; an examination group's rows are read-only, take no hold and share one output type; a lane group's rows are a lane's and reach the host | `every_row_fits_the_kind_of_its_group`, over `for_each_method!` with each row's output type |
| A lane's row reaches the host; a row that reaches the host is in `Tier::Launch` or `Tier::Vision`; an annotating row is in `overlay` | `a_row_that_reaches_the_host_is_never_listed_by_default` |
| Every `Held` kind a row takes has its owner and its release in the table of §1 | `every_hold_has_a_release`: each kind is released by a row or is in the test's list of kinds only a lease's end releases (`clock`, `build_status`) |
| The rows that change the app are exactly a list | `only_these_rows_change_the_app`, which replaces `what_ends_the_app_is_destructive_and_reads_never_act`: the acting and destructive rows by name, and the destructive ones `app.reload` and `session.close`, so a read declared to act is a line of the diff |
| `repeatable` and `resendable` follow the effect and the hold | `a_row_that_takes_a_hold_is_never_sent_twice`, a table over every effect and every `Hold` |
| A name `BREAKING` respelled is a method under the last name its chain reaches, and under no earlier one | `every_respelled_method_is_a_method_under_its_latest_name`, over every entry of `BREAKING`. It replaces the assertions of `the_ops_protocol_0_5_renamed_are_methods_now` and `protocol_0_5_renames_every_0_4_operation_into_the_method_table` that `windows.list`, `semantics.read`, `semantics.act` and `motion.set` are methods; the second test keeps its check that every 0.4 operation is listed |
| The schema is 0.33's golden, and differs from 0.32's only by what `BREAKING` lists | `the_wire_schema_is_the_one_its_version_published`, `every_published_schema_is_additive_to_the_next` |
| Every wire name is lower snake case | `every_wire_name_is_lower_snake_case`, over the schema's property names and enumerated strings, with the values that are another standard's (W3C key names) listed in the test; `every_vocabulary_enum_is_spelled_as_it_is_named` |
| A duration or a time names its unit | `a_duration_or_a_time_names_its_unit`, over the schema: a numeric property named for a time word ends `_ms`, `_us` or `_ns`, but for `t` and `arrival`. It sees a field by its name, so a time under another word is not seen |
| A read states its bound | the compiler: a trait in the test file, `BoundedRead`, implemented for each read-only marker by setting its typed `max_…` field or declaring it fixed-size; `every_read_names_its_bound_or_is_fixed_size` walks `for_each_method!` and does not build for a read without the impl or an impl that names a field its parameters lack |
| A journal is read by `since` and answers `entries` and `dropped` | `every_journal_is_read_since_a_sequence_number`, over `input.journal` and `window.announcements` |
| Every event name is `group.name` with a group of `GROUPS` | `every_event_is_named_for_a_group_of_the_table` |
| The core set and its bound | `the_core_set_is_the_thirteen_tools_mcp_lists_by_default`, with the new names |
| Hints follow the effect, the hold and the reach | `tool_hints_follow_the_effect_the_hold_and_the_reach` |
| A lease's end restores the clock's rate, ends its span, withdraws its banner and releases its input, before the drain's commands | `flui-runtime`'s `a_lease_that_goes_releases_everything_it_left_in_the_window`; `a_spans_stop_reports_the_callers_span_alone`; `the_banner_is_withdrawn_with_the_lease_that_reported_last` |
| Observers together hold at most the runtime's recordings less one connection's | `flui-runtime`'s `a_driver_starts_a_recording_while_observers_hold_their_share` |
| A method verb of `Tester` is named for its row, and every window method has its verb or the reason it has none | `flui-testing`'s `every_method_verb_is_its_methods_member`: a table made by a macro from each verb's own call, over a connection that records the methods sent. It fails a listed verb that sends another row or carries another name, and a window method with no entry |
| `window.environment` answers the root's values, and `text_sizing` its policy on each host | `flui-runtime`'s `a_windows_environment_is_what_its_root_media_query_holds`, with a fixed, a linear and an exact policy |
| Every service answers `app.build` bounded; one handed no reading refuses `app.config` and does not list it | `flui-runtime`'s `a_service_cuts_its_description_and_refuses_a_configuration_it_was_not_given`; `flui-testing`'s `the_headless_service_claims_no_configuration` |
| The hello names the application, not its window | `flui-app`'s `the_hello_names_the_application_not_its_window` |
| `app.config` sends no path, and a hook as its name alone | the types `StorageDir` and `InstalledHook`; `flui-app`'s `a_configured_storage_directory_is_answered_as_custom` |
| A new string field is redacted in a trace or named as kept | `flui-driver`'s `every_text_field_of_the_wire_is_redacted_or_named_as_kept` |
| A trace from before a break is refused | `flui-driver`'s `a_trace_from_before_a_break_is_refused_with_both_versions` |
| A read links nothing into a release binary | the `size` job, and the reader of a change to `ui_runtime` (ADR-0402) |
| `flui mcp` answers `server/discover` with its four revisions, serves `initialize` at the revision it negotiates, and refuses any other with the list | the adapter's `the_server_discovers_its_revisions`, `an_initialize_is_served_at_a_handshake_revision`, `an_unknown_revision_is_refused_with_the_supported_list` |
| Every row is a tool, in order, unless its hold has no place in MCP; the default list is the core; an observer's list holds nothing that changes the app; the list is the same before and after the app connects | the adapter's `every_row_is_a_tool_or_its_hold_is_not_offered`, `the_default_list_is_the_core_set_and_each_cap_adds_its_tier`, `an_observers_list_holds_only_what_an_observer_may_call`, `the_tool_list_is_the_same_before_and_after_the_app_connects` |
| The default list starts no process and reaches no other endpoint | the adapter's `the_default_list_holds_no_row_that_reaches_the_host`; `a_recording_keeps_text_only_where_the_server_was_started_to` |
| A call that reached a connection that then broke goes again only when its row is `resendable` | `flui-cli`'s `a_call_whose_connection_broke_goes_again_only_when_it_leaves_the_app_as_it_was`, whose rows change with the rule: a read is sent again; an act, and `overlay.show_build_status`, which the test sends again today, are not |
| A redial that lost held state is said once, and nothing is sent before it | the adapter's `a_call_after_a_lost_connection_answers_what_was_held_and_sends_nothing`; `a_call_after_a_lost_connection_that_held_nothing_goes_again` |
| A recording's rows reach its file, a lost one is answered incomplete once, and a number is never used twice | the adapter's `a_recordings_rows_are_written_as_they_arrive`, `a_recording_whose_connection_ended_stops_incomplete`, `a_recording_number_from_before_a_redial_names_nothing_later` |
| Events reach a client only as updates of the resources its stream named, one pending for each | the adapter's `a_listening_client_gets_one_update_for_a_burst`, `an_event_no_stream_asked_for_is_not_subscribed_to` |
| A sequence that leaves input held is refused by a front end that cannot hold it | `flui-protocol`'s `a_sequence_says_what_it_leaves_held`; the CLI's and the adapter's `a_sequence_that_leaves_input_held_is_refused_unsent` |
| Each resource is a read of a window or of the app; each prompt names tools that exist | `every_resource_is_a_read_of_a_window_or_of_the_app`, `every_prompt_names_tools_the_table_has` |
| Every row has its CLI path, a row that releases a hold excepted | the CLI's coverage test, with `_` as `-` |

No name is left under an alias. After the change that builds §3 to §5, `rg` of each old method
name of §3's table, each old event name of the `BREAKING` entry and each old `Tester` verb,
across `crates/`, `packages/`, `src/`, `tests/`, `examples/`, `tools/`, `.github/` and `docs/`,
finds them only in `docs/adr/`, in `crates/flui-protocol/src/version.rs` and under
`crates/flui-protocol/tests/schema/`. After the change that builds §7 the same holds for
`flui_windows`, `flui_read`, `flui_locate`, `flui_act`, `flui_wait`, `flui_motion` and
`flui_reload`.

## Pros and cons of the options

### 1. Keep the groups and respell the few that clash

- Good, because the fewest names change.
- Bad, because no rule says where the next method goes: `semantics`, `capture`, `run` and
  `build` each name a different kind of thing.

### 2. Three subject groups only

- Good, because the rule is one question.
- Bad, because every instrument is per window, so `window` would hold the clock, the audits,
  the spans, the trees and the waits, as the DevTools protocol's `Page` domain holds
  navigation, capture, lifecycle and emulation at once.

### 3. A closed table of groups, each of a stated kind

- Good, because BiDi's modules are of these kinds (`browser`, `browsingContext`, `session`;
  `input`, `emulation`, `log`), so the shape is the field's.
- Good, because a kind is a property of the rows (a hold, an output type, the side that
  serves), so a test places a row and two readers agree.
- Bad, because a group cannot be made by the change that needs it: it takes a record.

### 4. Instrument groups only

- Good, because it is the DevTools protocol's model, the largest in use.
- Bad, because the three things a caller addresses lose their names: a screenshot, a snapshot
  and a placement of one window would sit in three domains.

### 5. `snake_case`

- Good, because a Rust identifier is its wire name, and a recording, a trace and a schema read
  as the code does.
- Good, because MCP tool names are snake case in every server read (Playwright, Chrome
  DevTools, Dart), as is MCP's own `notifications/tools/list_changed`.
- Good, because nothing but the names of §3 and §5 changes in the recordings under
  `crates/flui-native/tests/native_recording/` and the golden trace.
- Bad, because the protocols FLUI's shape comes from (BiDi, the DevTools protocol, LSP, DAP)
  are camel case, so a reader who knows them sees `perform_actions` for `performActions`.

### 6. `camelCase`

- Good, because it is what BiDi, the DevTools protocol, LSP, DAP, Appium and MCP's own fields
  use.
- Bad, because every field of every wire type and every enumerated value would be respelled,
  in every recording and trace on disk.
- Bad, because the wire and the MCP tool would then differ, or tools would be camel case where
  no other server's are.

### 7. The window keeps what a client set

- Good, because a clock paused by one `flui drive` command is still paused for the next.
- Bad, because nothing restores it: a model pauses the clock, its server exits, and the
  developer's app stays frozen with no connection left that knows.
- Bad, because the clock and the banner then follow one rule and held input, a recording and a
  later override another, so a client that reconnects holds half of what it set.

### 8. The connection owns everything it leaves

- Good, because there is one rule and one exit, the one held input already has and its tests
  hold.
- Good, because a front end can account for a loss: what a connection held is a set of kinds
  the rows name.
- Bad, because a setting does not survive between two commands of a front end that connects
  for each.

### 9. The driver's seat keeps its settings for a grace period

- Good, because a client that reconnects in time finds what it set.
- Bad, because the period is a deadline somebody must serve while no conversation exists, and
  a second driver that arrives within it inherits a clock it did not pause.

### 10. One tool per method, a core set and tiers the person grants

- Good, because each tool has its own schema and its own true hints.
- Good, because what reaches beyond the app is listed only where a person asked for it.
- Good, because the default list stays at thirteen; Playwright MCP's `--caps` is the same
  device (`README.md:415`).
- Bad, because a model cannot turn a tier on for itself.

### 11. One tool per method, all listed

- Good, because nothing is hidden.
- Bad, because the list is 46 tools today and near 80 with the spec's, on every turn.
- Bad, because every server would list the tools that start a process and send input through
  the operating system.

### 12. An umbrella tool

- Good, because the list is one entry.
- Bad, because one tool has one annotation, one schema and every method's authority: the Dart
  server's `flutter_driver_command` is marked read-only while it taps, and takes
  `additionalProperties: true` (`lib/src/mixins/dtd.dart:1311`, `:1313`).

### 13. 2026-07-28 alone

- Good, because there is one framing.
- Bad, because a client that opens with `initialize` gets an error and no tools, and has no
  way forward (`basic/versioning.mdx`, the compatibility matrix).

### 14. The handshake revisions alone

- Good, because it is what is shipped.
- Bad, because those revisions have no stream a server may send a resource's change on without
  a session, and the current revision is the one new clients speak.

### 15. Both

- Good, because every client finds its framing, and the revision describes a server that
  answers both.
- Bad, because two framings are kept and tested.

### 16. Reads of what the runtime and the service hold

- Good, because nothing is invented and each read says where its value lives.
- Bad, because what lives in a package's subtree (a theme) waits for a generic read.

### 17. A registry packages fill

- Good, because `app.theme` would exist.
- Bad, because a theme is not the app's: two subtrees can hold two, and the registry would
  answer one of them.
- Bad, because it is new process-wide state every package must remember to fill, a copy that
  can miss its source.

## More information

- MCP 2026-07-28, at the commit in Sources: `server/tools.mdx` for the rules on names (`:65`,
  the list must not vary by connection; `:71`, a deterministic order; `:688`, stateful tools
  and explicit handles), hints, structured content and the two kinds of error;
  `basic/patterns/subscriptions.mdx` for what a stream carries; `basic/versioning.mdx` for a
  server that answers both eras.
- Playwright MCP marks every tool that is not read-only destructive
  (`microsoft/playwright@b0e0b7b555` `packages/playwright-core/src/tools/utils/mcp/tool.ts:38`),
  which leaves a client nothing to tell a click from a close by; this record marks only the
  `destructive` effect.
- Revisit the core set when the log journal exists: `log.entries` is the next read a model
  needs by default. Revisit `wait.settled` and `wait.frames`, which are `wait.until` with a
  fixed condition, when the kit no longer calls them by name. Revisit the handshake revisions
  when the clients in use speak 2026-07-28.
