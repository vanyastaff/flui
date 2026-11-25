# ADR-0403: A protocol method is named for the subject it addresses or the instrument it works, and one name serves the wire, the kit, the CLI and MCP

- **Status:** Accepted (2026-11-24)
- **Date:** 2026-11-24
- **Supersedes:**
  [ADR-0238](ADR-0238-flui-driver-protocol.md) §1's form of a wire name (`group.verb`) and its
  clause on what the MCP tool list is made from, only (§2, §7 here); §9's tier names, core set
  and bound of 13, only (§7); and the method names §10 and its Consequences spell (§3).
  [ADR-0288](ADR-0288-the-development-agent-is-one-sans-io-service.md) §9, §10 and §11: the
  names `input.release`, `record.start`, `record.stop` and the event names `input`, `record` and
  `record_native`, only (§3, §5); what the methods do is unchanged.
  [ADR-0395](ADR-0395-a-rebuild-is-shown-over-the-app-it-will-replace.md) §2: the name
  `build.report`, only (§3).
  [ADR-0401](ADR-0401-flui-run-serves-its-reload-at-an-endpoint-the-session-file-names.md) §1 and
  §4: the method name `run.reload` and the tool name `flui_reload`, only (§3, §7); §3's
  `run.reload` event of `flui run --json` keeps its name.
  [ADR-0400](ADR-0400-a-test-is-one-tester-over-a-connection-and-every-claim-has-one-instrument.md)
  §1's clause that `Placement` carries the platform convention (§6), and §2's and §5's verbs
  `enter_text` and `settle`, only (§3).
- **Related:** [ADR-0095](ADR-0095-agent-protocol-schema-crate.md) (the schema crate and its
  versioning rule); [ADR-0080](ADR-0080-agent-protocol-desktop-contract.md) (the error object);
  [ADR-0239](ADR-0239-every-debug-app-hosts-the-driver-agent.md) (which builds serve the agent);
  [ADR-0402](ADR-0402-a-binary-links-the-development-agent-only-where-a-hook-is-installed.md)
  (how a new read reaches the runtime); [ADR-0241](ADR-0241-driver-trace-format.md) (a trace
  names methods); [ADR-0325](ADR-0325-a-context-is-bound-as-ctx-and-a-description-builder-has-no-with.md) (the naming rules of the Rust API); [ADR-0242](ADR-0242-typed-inspect-and-tracing-conventions.md) (the inspected node `inspect.inherited` answers)
- **Sources:** W3C WebDriver BiDi, <https://w3c.github.io/webdriver-bidi/>; the Chrome DevTools
  Protocol, `ChromeDevTools/devtools-protocol` `json/browser_protocol.json`; MCP revision
  2026-07-28, <https://modelcontextprotocol.io/specification/2026-07-28>;
  `microsoft/playwright-mcp` `README.md`; `dart-lang/ai` `pkgs/dart_mcp_server/README.md`;
  `ChromeDevTools/chrome-devtools-mcp` `docs/tool-reference.md`; Appium's XCUITest and
  UiAutomator2 drivers, `docs/reference/execute-methods.md`; each read at its default branch on
  2026-11-24

## Context and problem statement

`flui-protocol`'s method table (`crates/flui-protocol/src/method.rs`) holds 46 methods in 16
groups, and the driver spec plans about 30 more. The groups were named as each method arrived.

| Fact | Evidence |
|------|----------|
| The groups mix four kinds of word: a subject (`window`, `element`, `session`), a technique (`semantics`), a verb (`capture`, `record`, `inspect`) and the tool that serves (`run`) | `method.rs:455-602` |
| One subject has two spellings | `windows.list` beside `window.placement` |
| `motion` names the clock on the wire and the reduced-motion policy in the framework | `motion.set`; `MediaQueryData::motion`, `AppConfig::motion_override` |
| Two rows differ by a value the protocol already has a type for | `capture.presented`, `capture.rendered`; `CaptureSource` |
| The shipped MCP server lists seven hand-written tools named `flui_read`, `flui_act`, …; the table's own tool names are listed nowhere | `crates/flui-cli/src/agent/mcp.rs:148-255`; `MethodRow::tool_name` |
| Event names are bare words, and one of them, `window`, means opened or closed only | `crates/flui-protocol/src/events.rs` |
| Nothing reads the application's configuration, its build or a window's environment, though the window dispatch already reads the root media query | `crates/flui-runtime/src/ui_runtime/agent_serve.rs:398-421` |
| The current MCP revision has no session and forbids a tool list that varies by connection | <https://modelcontextprotocol.io/specification/2026-07-28/server/tools> |
| Before the first release a wire name may be respelled once, in a version `BREAKING` lists | `crates/flui-protocol/src/version.rs` |

Scope: `flui-protocol`'s names and conventions, the reads it gains, and the MCP adapter. What
each existing method does is not changed here.

Which groups does the protocol have, how is a method placed and named, and how does the table
become an MCP server?

## Decision drivers

- A method that arrives later has one obvious group, found by a procedure, and no group is
  renamed for it.
- A name is the term its field uses, with the source that shows it (AGENTS.md, "Code for the
  next reader").
- One name on the wire, in the Rust API, in the CLI and in MCP: none of the four keeps a
  spelling of its own (the driver spec's requirement that all front ends come from one table).
- The break is made once. No alias outlives it.
- A model reads the tool list on every turn: a name must say what it does without the others,
  and the list must be small by default.
- A read reaches the runtime without linking the agent into a release binary (ADR-0402).
- A rule is held by a table test or a schema, not by a reader's memory (ADR-0303).

## Considered options

Groups:

1. Keep the groups and respell the few that clash.
2. Three subject groups only: `app`, `window`, `element`.
3. Subject groups and instrument groups (chosen).
4. Instrument groups only, as the DevTools protocol's domains.

Spelling on the wire:

5. `snake_case` (chosen).
6. `camelCase`.

The MCP server:

7. One tool per method, a core set listed by default and the rest by capability group (chosen).
8. One tool per method, all listed.
9. An umbrella tool that takes the method's name.

Reads of the application's state:

10. Reads served from what the runtime and the service hold today, at the scope that holds
    each (chosen).
11. App-level reads of whatever a test wants, `app.theme` among them, through a registry
    packages fill.

## Decision

Chosen: 3, 5, 7 and 10, because together they give every method a home that follows from what
it addresses, keep one spelling from the Rust type to the tool, and add no state to the runtime.

### §1. A group is a subject or an instrument

A **subject** is something a caller addresses. There are four:

| Group | Is | Holds today | Will hold |
|-------|----|-------------|-----------|
| `session` | the client's connection to one target | `attach`, `capabilities`, `close` | — |
| `app` | the application: its process, its build, its launch configuration, and what every window of one UI runtime shares | `launch`, `reload`, `build`, `config` | `background`, `activate`, `terminate`, `open_url`, `lifecycle`, `capabilities` |
| `window` | one window | `list`, `placement`, `environment`, `snapshot`, `screenshot`, `announcements` | `capabilities`, `close`, `activate`, `resize` |
| `element` | one element, named by a target | `locate`, `actionability`, the gestures, `type_text`, `press_keys`, `perform_action` | — |

An **instrument** is a development facility with state of its own that outlives a call (held
input, a clock, a recording, a span, a subscription, an override) or a result vocabulary of its
own (findings, inspected nodes). Its group is the word its field uses for it.

| Group | Is | Holds today | Will hold |
|-------|----|-------------|-----------|
| `input` | the input devices a client plays | `inject`, `perform_actions`, `release_actions`, `journal` | — |
| `wait` | a request held until a condition answers | `until`, `settled`, `frames` | — |
| `events` | the connection's event streams | `subscribe`, `unsubscribe` | — |
| `clock` | a window's animation clock | `set` | `state` |
| `overlay` | what development tools draw over a window | `show_build_status` | `show_performance`, `highlight` |
| `inspect` | the framework's own trees | `widget_tree`, `render_tree`, `layer_tree` | `inherited` |
| `audit` | rule checks that answer findings | `accessibility`, `screen` | `layout`, `text_scale`, `alternatives` |
| `perf` | frame performance over a span | `start`, `stop`, `expect` | `tag` |
| `recording` | a recording of what a runtime applies | `start`, `stop` | — |
| `navigation` | a window's route stack | — | `navigate`, `stack` |
| `emulation` | overrides of what the platform reports | — | `set_preferences` |
| `layout`, `pixels`, `baseline`, `trace`, `explore`, `report` | the driver spec's instruments | — | as the spec lists them |
| `os`, `device` | the app as the operating system shows and drives it; the devices it runs on | — | `os.snapshot`, `os.screenshot`, `os.perform_actions`; `device.list` |
| `log`, `clipboard`, `storage`, `display`, `assets`, `permission`, `network` | reserved | — | when the runtime has something to serve |

A contributor places a new method by these steps, taking the first that applies:

1. It goes through the operating system, not through the app: `os`, or `device` for the device.
2. It draws over a window and changes nothing else (`MethodEffect::Annotates`): `overlay`.
3. It works an instrument of the table: that instrument.
4. It introduces an instrument (state of its own, or a result vocabulary of its own): a new
   group, named with its source, added to the table of groups in the same change.
5. Otherwise it reads or changes what a user would recognise as the app's, a window's or an
   element's: the narrowest of `element`, `window`, `app` that holds it.
6. It is about the client's own connection: `session`.

Who serves a method, what it can change and where MCP lists it are columns of its row
(`ServedBy`, `MethodEffect`, `Tier`), never its group, with two fixed ties: a lane's rows are
`os` and `device` and no others, and an annotating row is `overlay` and no other.

Three things every row states in its documentation:

- **Its window.** A method of a window names it in its parameters, or its target does;
  parameters that name none address the app's only window (`Addressed`). State one UI runtime
  holds for all its windows is `app`'s, addressed through any of them, and the row says so.
- **Its role.** `ClientRole::may_call` reads the effect alone: an observer calls what reads or
  annotates. No group is closed to a role.
- **What it leaves behind.** State a call leaves has one owner. The connection owns held input
  (its lease, ADR-0288 §9), a recording, a subscription and an override, and each goes when the
  connection does; the window owns the clock's rate and an overlay, which stay.

### §2. How a name is formed

```text
group.member          lower snake case; the group is one word with no underscore
```

- A subject group is singular. A group that is a stream or a medium is plural or a mass noun:
  `events`, `pixels`.
- An action's member is a verb, with its object where the verb alone is ambiguous.
- A read's member is what it returns. The read of a collection of handles is `list`.
- A span is `start` and `stop`; a stream is `subscribe` and `unsubscribe`.
- The term the field shares wins; where the field splits, FLUI keeps its own word when three
  ecosystems use it. A changed or new name is recorded with its source.
- The marker type is the name in upper camel case (`ElementPerformAction`), the `Tester` verb
  is the member (`type_text`), or `group_member` where the member alone is not a verb
  (`wait_settled`), the CLI path is `flui drive <group> <member>` with `_` spelled `-`, and the
  MCP tool is `group_member`.

### §3. The names

Protocol 0.33 respells 19 methods, folds `capture.rendered` into `window.screenshot`, and adds
three reads. `METHODS` after it, in its order:

```rust
/// What was built and is running: its name, version, profile, target and versions.
AppBuild = "app.build", Agent, ReadOnly, App,
    $crate::session::Empty => $crate::app::AppBuild;
/// What the application was configured with at launch.
AppConfig = "app.config", Agent, ReadOnly, App,
    $crate::session::Empty => $crate::app::AppConfigReading;
/// Start an app with the agent attached, or start the session's app again.
AppLaunch = "app.launch", Driver, Acts, Core,
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
ClockSet = "clock.set", Agent(window), Acts, App,
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
EventsSubscribe = "events.subscribe", Agent, ReadOnly, Record, …;
EventsUnsubscribe = "events.unsubscribe", Agent, ReadOnly, Record, …;
InputInject = "input.inject", Agent(window), Acts, Input, …;
InputJournal = "input.journal", Agent(window), ReadOnly, Input, …;
/// Input by the path the policy chooses.
InputPerformActions = "input.perform_actions", Driver, Acts, Input,
    $crate::input::InputParams => $crate::act::ActOutcome;
/// Release every contact and key the connection's lease holds.
InputReleaseActions = "input.release_actions", Agent(window), Acts, Input,
    $crate::session::WindowParams => $crate::input::Released;
InspectLayerTree = "inspect.layer_tree", Agent(window), ReadOnly, Inspect, …;
InspectRenderTree = "inspect.render_tree", Agent(window), ReadOnly, Inspect, …;
InspectWidgetTree = "inspect.widget_tree", Agent(window), ReadOnly, Inspect, …;
/// Show a window where the application's next build is.
OverlayShowBuildStatus = "overlay.show_build_status", Agent(window), Annotates, App,
    $crate::overlay::BuildStatusParams => $crate::session::Empty;
PerfExpect = "perf.expect", Driver, ReadOnly, Perf, …;
PerfStart = "perf.start", Agent(window), ReadOnly, Perf, …;
PerfStop = "perf.stop", Agent(window), ReadOnly, Perf, …;
RecordingStart = "recording.start", Agent, ReadOnly, Record, …;
RecordingStop = "recording.stop", Agent, ReadOnly, Record, …;
SessionAttach = "session.attach", Driver, Acts, App, …;
SessionCapabilities = "session.capabilities", Driver, ReadOnly, App, …;
SessionClose = "session.close", Driver, Destructive, Core, …;
WaitFrames = "wait.frames", Agent(window), ReadOnly, App, …;
WaitSettled = "wait.settled", Agent(window), ReadOnly, App, …;
WaitUntil = "wait.until", Agent(window), ReadOnly, Core, …;
/// The announcements spoken since a sequence number.
WindowAnnouncements = "window.announcements", Agent(window), ReadOnly, Record,
    $crate::events::AnnouncementsParams => $crate::events::AnnouncementJournal;
/// What the runtime installs above the window's root: the preferences in effect and the
/// platform convention.
WindowEnvironment = "window.environment", Agent(window), ReadOnly, App,
    $crate::session::WindowParams => $crate::window::Environment;
/// The app's windows.
WindowList = "window.list", Agent, ReadOnly, Core,
    $crate::session::Empty => $crate::window::Windows;
WindowPlacement = "window.placement", Agent(window), ReadOnly, App, …;
/// A window's pixels: the frame it last presented, or its layer tree rendered again.
WindowScreenshot = "window.screenshot", Agent(window), ReadOnly, Core,
    $crate::window::ScreenshotParams => $crate::window::Screenshot;
/// A window's semantics tree, or the part of it under one element.
WindowSnapshot = "window.snapshot", Agent(window), ReadOnly, Core,
    $crate::tree::ReadParams => $crate::tree::Tree;
```

A row ending `…` keeps its parameters and output. Around the rows:

- `window.screenshot` takes `source: Option<CaptureSource>`. Absent, it answers the presented
  frame, or the rendered one where a surface cannot be copied, as `capture.presented` does;
  named, it answers that source or `not_supported`. The answer's `source` says which.
- `Tier::Session` becomes `Tier::App`, and `MAX_CORE` is 14: the core set gains `app.reload`.
- The output of `window.list` is `Windows`, so no two public items are `WindowList`.
- `Tester::enter_text` becomes `type_text`, `pump_and_settle` `wait_settled`, `semantics`
  `snapshot`, `inspect_widgets` and `inspect_render` gain `_tree`, and `foreground` becomes
  `activate`; each `try_` twin follows.
- `version::BREAKING` gains the entry for 0.33: the 19 method names old then new, and under
  `removed` the schema paths of `capture.rendered`, of the event names of §5, of
  `EventFilter`'s `kind`, of `Matches`' `truncated` and of `Placement`'s `platform`.

What changed, and the term each new name takes:

| Was | Is | The field's term |
|-----|----|------------------|
| `audit.a11y` | `audit.accessibility` | Appium `mobile: performAccessibilityAudit`; the DevTools protocol's `Accessibility` |
| `build.report` | `overlay.show_build_status` | the DevTools protocol's `Overlay` domain; the "error overlay" ADR-0395 cites |
| `capture.presented`, `capture.rendered` | `window.screenshot` | BiDi `browsingContext.captureScreenshot` and the DevTools protocol's `Page.captureScreenshot`, each with the source as a parameter; Playwright `page.screenshot()` |
| `element.scroll_until` | `element.scroll_until_visible` | Maestro, `flutter_test` and `flutter_driver` `scrollUntilVisible` |
| `input.perform`, `input.release` | `input.perform_actions`, `input.release_actions` | BiDi `input.performActions`, `input.releaseActions` |
| `inspect.widgets`, `inspect.render`, `inspect.layers` | `inspect.widget_tree`, `inspect.render_tree`, `inspect.layer_tree` | `flutter_driver`'s `get_render_tree`, `get_layer_tree`; the Dart MCP server's `get_widget_tree` |
| `motion.set` | `clock.set` | Compose `MainTestClock`; FLUI's `ClockReading` and `VirtualClock` |
| `record.start`, `record.stop` | `recording.start`, `recording.stop` | Maestro `startRecording`, `stopRecording`; Playwright MCP `browser_start_recording` |
| `run.reload` | `app.reload` | the Dart MCP server's `hot_reload`; BiDi `browsingContext.reload` |
| `semantics.act` | `element.perform_action` | Compose `performSemanticsAction`; Detox `performAccessibilityAction` |
| `semantics.announcements` | `window.announcements` | none: a journal read named by what it returns |
| `semantics.locate` | `element.locate` | BiDi `browsingContext.locateNodes`; WebDriver's `/element` |
| `semantics.read` | `window.snapshot` | Playwright `ariaSnapshot`, Playwright MCP `browser_snapshot`, Chrome DevTools MCP `take_snapshot` |
| `session.launch` | `app.launch` | Appium `mobile: launchApp`, Maestro `launchApp`, Detox `device.launchApp`, the Dart MCP server's `launch_app` |
| `windows.list` | `window.list` | Appium `mobile: listWindows`; MCP `tools/list` |

The 26 names that stay are the field's already (`tap`, `swipe`, `pinch`, `attach`, `subscribe`,
`actionability`) or FLUI's own where the field has none or splits (`placement`, `journal`,
`settled`, `until`).

### §4. Wire conventions

- **Spelling.** Groups, members, fields, enumerated values, event names and error codes are
  lower snake case. A Rust identifier is its wire spelling, and `serde`'s `rename_all` is the
  only mapping.
- **Units.** A number whose unit its type does not carry names the unit: `_ms` for a duration
  or a timeout, `_us` for a timestamp. Geometry is logical pixels by type (`LogicalRect`,
  `LogicalPoint`, `LogicalSize`); a count of device pixels says so in its documentation.
- **Bounds.** A read whose answer grows with the application's state takes a `max_…` parameter
  with a stated default and answers `truncated`. Where it can count what it left out, the count
  is `omitted` (`Matches::truncated`, a count, becomes `omitted`; a node keeps
  `omitted_children`). A read is narrowed by its parameters (`root`, a locator); there is no
  cursor.
- **Journals.** A bounded journal is read with `since` and `max_entries` and answers `entries`,
  oldest first, each with its `seq`, and `dropped`. The last `seq` is the next `since`.
- **Errors.** `ErrorCode` stays as it is. A code names the state that refused, never the method.
  A new method uses a code the list has before it adds one; a new code is additive, reads as
  `platform` in an older client, and is added by the record that needs it. `not_supported`
  names the missing `capability`.
- **Versions.** The break is one version. An app that speaks it refuses an older client
  `version`, naming both (`ProtocolVersion::serves`). No server accepts an old name. A trace
  names the protocol it was recorded in, and the trace reader refuses one a break separates
  from its own with the same error; the golden trace is recorded again.

### §5. Events are named as methods are

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

- An occurrence is a subject and a past participle (`opened`, `snapshot_changed`).
- A journal's event is `<entry>_added` and carries the entry its read returns, with the same
  `seq`.
- A read a client reads again on change has `<read>_changed`, which carries the revision and
  never the value.
- An item of a stream is the bare noun (`frame`, `row`, `gap`).

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
    pub text_scale: Option<f64>,
    pub font_weight_adjustment: i32,
    pub locales: Option<Vec<String>>,
    pub regional: RegionalFormats,
    pub accent_color: Option<Color>,
    pub scroll_bars: ScrollBars,
    pub view_insets: LogicalInsets,
    pub system_gesture_insets: LogicalInsets,
    pub overridden: Vec<EnvironmentField>,
}

// flui_protocol::app
/// What was built and is running.
#[non_exhaustive]
pub struct AppBuild {
    pub name: String,
    pub id: Option<String>,
    pub version: Option<String>,
    pub profile: BuildProfile,
    pub target: String,
    pub arch: String,
    pub pid: Option<u32>,
    pub flui: String,
    pub protocol: ProtocolVersion,
    pub records_native: bool,
}

/// What the application was configured with at launch.
#[non_exhaustive]
pub struct AppConfigReading { /* plain values; text bounded; a path never; a hook as a name */ }
```

- `window.environment` is a window method, served in the agent's window dispatch from the
  presentation's root media query and its platform convention, the values `window.placement`
  reads its size from. `Placement` loses `platform`; the size, scale and safe area stay there.
- `app.build` and `app.config` are the app's own methods, answered by the service from what
  `flui-app` hands it where it installs the agent, as it hands the hello's `AppDescription`
  today. `AppBuild::name` is the application's identity; the hello's `name`, today the window
  title, becomes the same.
- No path leaves the app: `AppConfig::storage_dir` is answered as `default` or `custom`. A
  field that can hold the user's text (`title`) is bounded to 256 bytes and is text a trace
  redacts unless it was recorded with `keep_text` (ADR-0215 §5). A hook is answered as whether
  it is installed.
- `AppBuild::version` is absent until the application can state its version; nothing holds it
  today.
- There is no `app.theme`. A theme is a value a package publishes down a subtree, which the
  runtime cannot name and which differs by element. Its home is `inspect.inherited`, the
  inherited values in effect at an element as inspected nodes, which needs a walk `flui-view`
  does not expose and an `Inspect` description the theme types do not have.

A read added later is a window method served in `agent_serve`, or a value `flui-app` hands the
service where the agent is installed. It adds no call from the runtime's own paths (ADR-0402
§1), states its scope, its bound and which of its fields can hold the user's text, and names the
event that says it changed.

### §7. MCP lists the table

`flui mcp` serves MCP revision 2026-07-28. Every method is a tool; none is written by hand.

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
}
pub struct ResourceRow {
    pub method: &'static str,
    pub uri: &'static str,
    pub updated_by: &'static [EventName],
    pub media_type: &'static str,
}
pub struct PromptRow {
    pub name: &'static str,
    pub tools: &'static [&'static str],
    pub text: &'static str,
}
```

| `MethodEffect` | `readOnlyHint` | `destructiveHint` | `idempotentHint` |
|----------------|----------------|-------------------|------------------|
| `read_only` | true | omitted | omitted |
| `annotates` | false | false | true |
| `acts` | false | false | false |
| `destructive` | false | true | false |

`openWorldHint` is true for `ServedBy::Lane` and false otherwise.

- **The list** is the rows `ToolSelection::lists`, in `METHODS`' order: the core tier, each
  tier `--caps` names, less what `--role observer` may not call, filtered by name. It is fixed
  when the server starts and is the same whichever app the server reaches; a tool the app does
  not answer fails `not_supported`.
- **Schemas.** `inputSchema` is the method's `Params`; `outputSchema` is its `Output` or
  `{ "error": ErrorObject }`. The result's `structuredContent` conforms, and its text is the
  serialized JSON, except that a snapshot's text is the outline, whose references are written
  whole (`w3/e12`), and a screenshot is an image block.
- **Targets.** A model reads `window_snapshot` and acts on a `Target`: a reference from the
  outline or a locator. An act answers a snapshot delta by default. A reference the app no
  longer knows fails `stale` or `gone` and is never bound again.
- **Errors.** An `ErrorObject` is a result with `isError`. A parameter that does not read as
  the method's `Params` is `invalid_argument`, returned the same way. A call with no answer
  whose method changes the app is an error with `effect: may_have_run` and is not sent again
  (ADR-0395 §4). Only an unknown tool or a malformed request is a JSON-RPC error.
- **Resources.** A read-only method whose parameters are a window or nothing may be a resource
  under `flui://<group>/<member>` or `flui://window/{window}/<member>`; `RESOURCES` lists
  them with the events that update each. Each is a tool as well.
- **Prompts.** `PROMPTS` lists scenarios a person picks; each names the tools it uses.
- **State.** The server keeps none per connection. Windows and elements are the app's handles;
  the app is the one the server process was started for.

### Consequences

- Good, because a later method is placed by six questions, and the rows the driver spec plans
  and the reads a test will ask for next each have a group before they are built.
- Good, because a model's tool list reads as the wire does, and the CLI, the kit and a trace
  spell a method one way.
- Good, because the reads of §6 cost the runtime no state and a release binary no code.
- Good, because an observer can be a second MCP server that only reads.
- Bad, because 19 names, 8 event names, 6 `Tester` verbs and a tier change at once: several
  hundred mentions across `crates/`, `docs/` and `changelog.d/`, most of them in
  `flui-protocol`, `flui-cli`, `flui-runtime` and `flui-driver`, the schema's next golden, the
  golden trace and seven records in `docs/adr/` (ADR-0095, 0238, 0241, 0288, 0395, 0400, 0401),
  each rewritten where it spells an old name, in the same change.
- Bad, because an app that speaks 0.33 refuses every older client, and a `flui` tool older than
  the app it finds says `version` and nothing more helpful.
- Bad, because `element.perform_action` and `input.perform_actions` differ by a letter and a
  group. They are in different tiers, and each description opens with the difference.
- Bad, because the theme, the resolved locale and the text direction stay unread until
  `inspect.inherited` is built.
- Neutral, because `flui run --json` keeps its own event names (`run.reload`, `build.phase`):
  that stream is the command's, not the protocol's.

### Confirmation

| Rule | Held by |
|------|---------|
| A name is `group.member` in lower snake case, a group has no underscore, the table is sorted, and a tool name splits back into its method | `flui-protocol`'s `every_method_is_named_group_dot_member_once_and_listed_in_order` (today's `every_method_is_named_group_dot_verb_once_and_listed_in_order`, with the group rule added) |
| The groups are the ones this record lists | `every_group_is_one_the_table_of_groups_names`: a table of each group with its kind, failing a row in a group it lacks |
| A lane's rows are `os` and `device`; an annotating row is `overlay` | `a_lanes_rows_and_an_overlays_rows_are_in_their_groups`, over `METHODS` |
| What 0.33 renamed is a method now, and its old name is none | `the_names_protocol_0_33_respelled_are_methods_now`, as `the_ops_protocol_0_5_renamed_are_methods_now` |
| The schema is 0.33's golden, and differs from 0.32's only by what `BREAKING` lists | `wire_schema` |
| Every event name is `group.name` with a group the table has | `every_event_is_named_for_a_group_of_the_table` |
| A read states its bound | `every_read_names_its_bound_or_is_fixed_size`: a table of each read-only method with its bound field, failing a read it lacks |
| The core set and its bound | `the_core_set_is_the_tools_mcp_lists_by_default` |
| Hints follow the effect and the side | `tool_hints_follow_the_effect_and_the_side_that_serves` |
| Every method is a tool, in order; the default list is the core; an observer's list holds nothing that changes the app; the list is the same before and after the app connects | the adapter's `every_method_is_a_tool_in_table_order`, `the_default_list_is_the_core_set_and_each_cap_adds_its_tier`, `an_observers_list_holds_only_what_an_observer_may_call`, `the_tool_list_is_the_same_before_and_after_the_app_connects` |
| Each resource is a read of a window or of the app; each prompt names tools that exist | `every_resource_is_a_read_of_a_window_or_of_the_app`, `every_prompt_names_tools_the_table_has` |
| Every method has its CLI path | the CLI's coverage test, with `_` as `-` |
| A `Tester` verb is its method's member | the compiler: the old verbs are deleted, not deprecated |
| `window.environment` answers the root's values, and an override shows in `overridden` | `flui-runtime`'s `a_windows_environment_is_what_its_root_media_query_holds` |
| `app.config` sends no path | `flui-app`'s `a_configured_storage_directory_is_answered_as_custom_and_never_as_its_path` |
| A read links nothing into a release binary | the `size` job, and the reader of a change to `ui_runtime` (ADR-0402) |

No name is left under an alias: `rg` of each old name across `crates/`, `packages/`, `docs/`,
each `ARCHITECTURE.md` and `AGENTS.md` finds only `version::BREAKING` and the published schema
files.

## Pros and cons of the options

### 1. Keep the groups and respell the few that clash

- Good, because the fewest names change.
- Bad, because no rule says where the next method goes: `semantics`, `capture`, `run` and
  `build` each name a different kind of thing.
- Bad, because the lane's planned rows (`input.os`, `capture.os`, `os.tree`) and the overlay's
  later ones would each be placed by taste.

### 2. Three subject groups only

- Good, because the rule is one question.
- Bad, because every instrument is per window, so `window` would hold the clock, the audits,
  the spans, the trees and the waits, as the DevTools protocol's `Page` domain holds navigation, capture, lifecycle and emulation at once.
- Bad, because a model would see thirty tools beginning `window_`.

### 3. Subject groups and instrument groups

- Good, because WebDriver BiDi's modules are these two kinds (`browser`, `browsingContext`,
  `session`; `input`, `emulation`, `log`, `network`, `storage`), so the shape is the field's.
- Good, because an instrument's state has one owner, and its group is where that owner's
  methods are.
- Bad, because the line between a window's state and an instrument needs a rule (§1, step 4)
  and a table test.

### 4. Instrument groups only

- Good, because it is the DevTools protocol's model, the largest in use.
- Bad, because the three things a caller addresses lose their names: a screenshot, a snapshot
  and a placement of one window would sit in three domains.

### 5. `snake_case`

- Good, because a Rust identifier is its wire name: no mapping can drift, and a recording, a
  trace and a schema read as the code does.
- Good, because MCP tool names are snake case in every server read (Playwright, Chrome DevTools,
  Dart, mobile-mcp), as is MCP's own `notifications/tools/list_changed`.
- Good, because nothing but the names of §3 changes in the published schemas' successor, the
  native recordings under `crates/flui-native/tests/native_recording/` and the golden trace.
- Bad, because the protocols FLUI's shape comes from (BiDi, the DevTools protocol, LSP, DAP)
  are camel case, so a reader who knows them sees `perform_actions` for `performActions`.

### 6. `camelCase`

- Good, because it is what BiDi, the DevTools protocol, LSP, DAP, Appium and MCP's own fields
  use.
- Bad, because every field of every wire type and every enumerated value would be
  respelled, in every recording and trace on disk.
- Bad, because the wire and the MCP tool would then differ (`performActions`,
  `input_perform_actions`), or tools would be camel case where no other server's are.

### 7. One tool per method, a core set and capability groups

- Good, because each tool has its own schema and its own honest hints.
- Good, because the default list stays near a dozen; Playwright MCP's `--caps` is the same
  device.
- Bad, because a model cannot turn a group on for itself: the person starts the server with it.

### 8. One tool per method, all listed

- Good, because nothing is hidden.
- Bad, because the list is 48 tools now and near 80 with the spec's, on every turn.

### 9. An umbrella tool

- Good, because the list is one entry.
- Bad, because one tool has one annotation and one schema: Dart's `flutter_driver_command` is
  marked read-only while it taps, and takes `additionalProperties: true`.

### 10. Reads of what the runtime and the service hold

- Good, because nothing is invented and each read says where its value lives.
- Bad, because what lives in a package's subtree (a theme) waits for a generic read.

### 11. A registry packages fill

- Good, because `app.theme` would exist.
- Bad, because a theme is not the app's: two subtrees can hold two, and the registry would
  answer one of them.
- Bad, because it is new process-wide state every package must remember to fill, a copy that
  can miss its source.

## More information

- Names and conventions of other protocols: WebDriver BiDi's `module.command` strings and its
  events (`log.entryAdded`, `browsingContext.contextCreated`); WebDriver Classic's endpoint
  table, <https://www.w3.org/TR/webdriver2/>; the DevTools protocol's domains (`Overlay`,
  `Emulation`, `LayerTree`, `Accessibility`); Playwright's `docs/src/api/class-locator.md`,
  `class-page.md` and `actionability.md`; Appium's `mobile:` methods (`launchApp`,
  `activateApp`, `backgroundApp`, `terminateApp`, `performAccessibilityAudit`, `listWindows`);
  Maestro's `YamlFluentCommand.kt` (`launchApp`, `scrollUntilVisible`, `startRecording`); Detox's
  `docs/api/device.md`; Flutter's `flutter_driver` command kinds (`get_render_tree`,
  `get_layer_tree`) and `flutter_test`'s `controller.dart`.
- MCP: the tools page for the rules on names, the list, hints, structured content, errors and
  stateful tools; the changelog for what 2026-07-28 removed (sessions, the handshake,
  `resources/subscribe`).
- Revisit the core set when the log journal exists: `log.entries` is the next read a model
  needs by default. Revisit `wait.settled` and `wait.frames`, which are `wait.until` with a
  fixed condition, when the kit no longer calls them by name.
