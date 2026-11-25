# ADR-0403: A protocol method is named for the subject it addresses or the instrument it works, and one name serves the wire, the kit, the CLI and MCP

- **Status:** Accepted (2026-11-24)
- **Date:** 2026-11-24
- **Supersedes:**
  [ADR-0238](ADR-0238-flui-driver-protocol.md) §1's form of a wire name (`group.verb`) and of a
  tool name (`group_verb`), only (§2, §7 here); §9's tier name `session` and the names its core
  set spells, only (§3; the set and its bound of 13 stand); and the method names §10 and its
  Consequences spell (§3).
  [ADR-0288](ADR-0288-the-development-agent-is-one-sans-io-service.md) §9, §10 and §11: the
  names `input.release`, `record.start`, `record.stop` and the event names `input`, `record` and
  `record_native`, only (§3, §5); what the methods do is unchanged.
  [ADR-0395](ADR-0395-a-rebuild-is-shown-over-the-app-it-will-replace.md) §2: the name
  `build.report`, only (§3).
  [ADR-0401](ADR-0401-flui-run-serves-its-reload-at-an-endpoint-the-session-file-names.md) §1 and
  §4: the method name `run.reload`, the tier name `session` and the tool name `flui_reload`,
  only (§3, §7); §3's `run.reload` event of `flui run --json` keeps its name.
  [ADR-0400](ADR-0400-a-test-is-one-tester-over-a-connection-and-every-claim-has-one-instrument.md)
  §1's clause that `Placement` carries the platform convention (§6); §2's verbs `enter_text` and
  `scroll_until` and §5's `Tester::semantics` (§3); and the method name `audit.a11y` in §3 and
  §5 (§3). Its `settle`, `Settled` and `wait_settled` stand.
  [ADR-0095](ADR-0095-agent-protocol-schema-crate.md) §3's names of `flui mcp`'s tools and of the
  operations they call, only (§3, §7).
  [ADR-0176](ADR-0176-presentation-animation-playback.md)'s sentence that the operation is
  exposed as `motion`, only (§3).
  [ADR-0241](ADR-0241-driver-trace-format.md) §5's sentence that `flui_trace` is the one version
  a reader checks, and the name `record.start` in its Consequences, only (§3, §4).
- **Related:** [ADR-0080](ADR-0080-agent-protocol-desktop-contract.md) (the error object);
  [ADR-0215](ADR-0215-input-recording-and-backend-conformance.md) (the input recording, and
  which text a recording keeps); [ADR-0239](ADR-0239-every-debug-app-hosts-the-driver-agent.md)
  (which builds serve the agent); [ADR-0242](ADR-0242-typed-inspect-and-tracing-conventions.md)
  (the inspected node `inspect.inherited` answers);
  [ADR-0342](ADR-0342-a-value-added-to-a-closed-union-breaks-the-agent-protocol.md) (what a protocol version may
  remove or add);
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
| Event names are bare words, and one of them, `window`, means opened or closed only | `crates/flui-protocol/src/events.rs` |
| Nothing reads the application's configuration, its build or a window's environment, though the window dispatch already reads the root media query | `crates/flui-runtime/src/ui_runtime/agent_serve.rs:403` |
| The hello names the app by its window's title | `crates/flui-app/src/app/agent/served.rs:52` |
| Before the first release a wire name may be respelled once, in a version `BREAKING` lists | `crates/flui-protocol/src/version.rs`; ADR-0342 |

Scope: `flui-protocol`'s groups and names, its wire conventions, the names of its events, three
reads it gains, and the name of an MCP tool. No method's behaviour changes here: what a call
does, what it leaves in the app and who releases it stay as the code has them.

Not decided here, and owed its own record with the work that builds the CLI and the MCP
adapter: who owns what a call leaves on a connection (held input, a stream, a recording, a
span, a clock rate, a build status) when a front end's connection is shorter than that state;
which rows a front end lists beyond the tiers; MCP's hints, revisions, resources and streams;
and what a model may cause without its person's flag. The driver spec's CLI and MCP task lists
those questions.

Which groups does the protocol have, how is a method placed and named, and what does it read of
the application's state?

## Decision drivers

- A method that arrives later has one group, found by a procedure two readers apply the same
  way, and no group is renamed for it.
- A name is the term its field uses, with a pinned source that shows it (AGENTS.md, "Code for
  the next reader"; `docs/adr/README.md`, "Sources are primary").
- One name on the wire, in the Rust API, in the CLI and in MCP (the driver spec's requirement
  that all front ends come from one table).
- The break is made once. No alias outlives it.
- A read reaches the runtime without linking the agent into a release binary (ADR-0402), and
  answers only what something holds.
- A rule is held by a type or a table test, not by a reader's memory (ADR-0303).

## Considered options

Groups:

1. Keep the groups and respell the few that clash.
2. Three subject groups only: `app`, `window`, `element`.
3. A closed table of groups, each of a stated kind (chosen).
4. Instrument groups only, as the DevTools protocol's domains.

Spelling on the wire:

5. `snake_case` (chosen).
6. `camelCase`.

Reads of the application's state:

7. Reads served from what the runtime and the service hold, at the scope that holds each
   (chosen).
8. App-level reads of whatever a test wants, `app.theme` among them, through a registry
   packages fill.

The name of an MCP tool:

9. The method's name with its dot as `_` (chosen).
10. The wire name unchanged, dot included.
11. Names written for the tool list, as the seven shipped ones.

## Decision

Chosen: 3, 5, 7 and 9. Together they place every method by a stated procedure, keep one
spelling from the Rust type to the tool, and add no state to the runtime.

### §1. The groups are a closed table, each of a stated kind

The groups are a table in `flui-protocol`. A group joins it in a record that names its kind and
the source of its word.

```rust
// flui_protocol::method
#[non_exhaustive]
pub enum GroupKind {
    /// Something a caller addresses.
    Subject,
    /// A development facility that keeps state a call leaves in the app: held input and its
    /// journal, a stream, a clock's rate, what is drawn over a window, a span, a recording.
    Instrument,
    /// A family of reads with one output type. Every row is read-only.
    Examination,
    /// The app as the operating system shows and drives it. Every row is a lane's.
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
| `input` | instrument | the input a client plays, what of it is held, and the journal of what a window was delivered | `inject`, `perform_actions`, `release_actions`, `journal` |
| `events` | instrument | the connection's event streams | `subscribe`, `unsubscribe` |
| `clock` | instrument | a window's animation clock, the presentation's `MotionClock` (ADR-0176) | `set` |
| `overlay` | instrument | what development tools draw over a window | `show_build_status` |
| `perf` | instrument | frame performance over a span | `start`, `stop`, `expect` |
| `recording` | instrument | the input recording of ADR-0215: what a runtime applied, row by row | `start`, `stop` |
| `inspect` | examination (`InspectReading`) | the framework's own trees | `widget_tree`, `render_tree`, `layer_tree` |
| `audit` | examination (`AuditReport`) | rule checks that answer findings | `accessibility`, `screen` |
| `wait` | examination (`WaitOutcome`) | a request held until a condition answers | `until`, `settled`, `frames` |

Names reserved for groups that have no row yet. Each joins `GROUPS`, with its kind, in the
record that lands its first row:

| Group | For |
|-------|-----|
| `os`, `device` | the lane's rows: `os.snapshot`, `os.screenshot`, `os.perform_actions`, `os.compare_projection`; `device.list` and the device's lifecycle |
| `emulation` | overrides of what the platform reports, `emulation.set_preferences` first |
| `navigation`, `log`, `clipboard`, `storage`, `display`, `assets`, `permission`, `network` | a window's route stack; the app's log and failures; the device's clipboard, documents and displays |
| `layout`, `pixels`, `baseline`, `trace`, `explore`, `report` | the driver spec's facilities |

A contributor places a new method by these steps, taking the first that applies:

1. A lane serves it: `os`, or `device` for the device itself.
2. It only draws over a window (`MethodEffect::Annotates`): `overlay`.
3. Its output is an examination group's output type: that group.
4. It starts, stops, sets or releases state an instrument of the table keeps, or reads what
   that instrument holds or recorded: that instrument. A journal belongs to the instrument one
   of whose methods produces its entries (`input.inject` makes the entries `input.journal`
   reads), and to the subject where no method does (`window.announcements`: nothing speaks an
   announcement but the app).
5. It works one of the facilities the table of reserved names lists: that group, which the
   record that lands the row adds to `GROUPS` with its kind.
6. It is about the client's own connection: `session`.
7. Otherwise it reads or changes what a user would recognise as the app's, a window's or an
   element's: the narrowest of `element`, `window`, `app` that holds it.

A read of what the app sees lives with the subject, whatever set it (`window.environment`). The
clock has no separate read: `clock.set` with neither field answers its state, as ADR-0176
decided. Who serves a method, what it changes and where a front end lists it are columns of
its row (`ServedBy`, `MethodEffect`, `Tier`), never its group.

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
  a collection of handles is `list`. The one read named by its verb is `element.locate`: what
  it returns is whatever the locator matches, and `locate` is the field's word for it.
- A span is `start` and `stop`; a stream is `subscribe` and `unsubscribe`.
- The term the field shares wins; where the field splits, FLUI keeps its own word when three
  ecosystems use it. A changed or new name is recorded with its pinned source.
- The marker type is the name in upper camel case (`ElementPerformAction`). A method's CLI path
  is `flui drive <group> <member>` with `_` spelled `-`, and its MCP tool is `group_member`
  (§7). A `Tester` verb follows §3. A shorthand a front end has beside a method's own path
  (`flui drive launch`) is that front end's.

### §3. The names

Protocol 0.33 respells 19 methods, folds `capture.rendered` into `window.screenshot`, respells
the events (§5) and adds `window.environment` (§6). `METHODS` after it and after the two reads
of §6 that join it later, in its order:

```rust
/// What is running: the application's name and identifier, its version, the host's build and
/// target.
AppBuild = "app.build", Agent, ReadOnly, Control,
    $crate::session::Empty => $crate::session::AppDescription;
/// What the application was configured with when it was last composed.
AppConfig = "app.config", Agent, ReadOnly, Control,
    $crate::session::Empty => $crate::app::AppConfigReading;
/// Start an app with the agent attached.
AppLaunch = "app.launch", Driver, Acts, Core,
    $crate::session::LaunchParams => $crate::session::Attached;
/// Rebuild the application and apply the build at a tier, at least the one asked for.
AppReload = "app.reload", Supervisor, Destructive, Control,
    $crate::reload::ReloadParams => $crate::reload::ReloadReport;
/// Accessibility rules over a window's published tree.
AuditAccessibility = "audit.accessibility", Agent(window), ReadOnly, Audit,
    $crate::audit::AuditParams => $crate::audit::AuditReport;
/// Every rule this build applies, saying what was checked and what was not.
AuditScreen = "audit.screen", Driver, ReadOnly, Core,
    $crate::audit::AuditParams => $crate::audit::AuditReport;
/// Change, step or read a window's animation clock (ADR-0176).
ClockSet = "clock.set", Agent(window), Acts, Control,
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
/// Release every contact and key the agent holds.
InputReleaseActions = "input.release_actions", Agent(window), Acts, Input,
    $crate::session::WindowParams => $crate::input::Released;
InspectLayerTree = "inspect.layer_tree", Agent(window), ReadOnly, Inspect, …;
InspectRenderTree = "inspect.render_tree", Agent(window), ReadOnly, Inspect, …;
InspectWidgetTree = "inspect.widget_tree", Agent(window), ReadOnly, Inspect, …;
/// Show a window where the application's next build is.
OverlayShowBuildStatus = "overlay.show_build_status", Agent(window), Annotates, Control,
    $crate::overlay::BuildStatusParams => $crate::session::Empty;
PerfExpect = "perf.expect", Driver, ReadOnly, Perf, …;
PerfStart = "perf.start", Agent(window), ReadOnly, Perf, …;
PerfStop = "perf.stop", Agent(window), ReadOnly, Perf, …;
RecordingStart = "recording.start", Agent, ReadOnly, Record, …;
RecordingStop = "recording.stop", Agent, ReadOnly, Record, …;
SessionAttach = "session.attach", Driver, Acts, Control, …;
SessionCapabilities = "session.capabilities", Driver, ReadOnly, Control, …;
SessionClose = "session.close", Driver, Destructive, Core, …;
WaitFrames = "wait.frames", Agent(window), ReadOnly, Control, …;
WaitSettled = "wait.settled", Agent(window), ReadOnly, Control, …;
WaitUntil = "wait.until", Agent(window), ReadOnly, Core, …;
/// The announcements spoken since a sequence number.
WindowAnnouncements = "window.announcements", Agent(window), ReadOnly, Record,
    $crate::events::AnnouncementsParams => $crate::events::AnnouncementJournal;
/// What the runtime installs above the window's root: the preferences in effect and the
/// platform convention. A value the host observed nothing for is the root's default.
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

A row ending `…` keeps its parameters and output, and every row keeps its side, its effect and,
but for the tier's name, its tier. Around the rows:

- **`window.screenshot`** takes `source: Option<ScreenshotSource>`, a type with the two sources
  an agent serves, `presented` and `rendered`. Absent, the window answers its presented frame,
  and a host that cannot copy its surface answers the rendered one; named, it answers that
  source or `not_supported`. The answer's `source` stays a `CaptureSource`, whose third value,
  `os`, is the lane's `os.screenshot` alone. No host answers either source yet.
- **Tiers.** `Tier::Session` becomes `Tier::Control`: it holds `session.attach`,
  `session.capabilities`, `window.placement`, the frame and settle waits, the clock, the build
  status and the reload, few of which are the `session` group's, and the reads of §6 join it.
  The core set is the thirteen it was, under their new names: `app.launch`, `audit.screen`,
  `element.locate`, `element.perform_action`, `element.press_keys`,
  `element.scroll_into_view`, `element.tap`, `element.type_text`, `session.close`,
  `wait.until`, `window.list`, `window.screenshot` and `window.snapshot`. Whether a tier
  should keep `app.launch` or `app.reload` from a model's default list is a question of what a
  model may cause, which the record named in "Context" decides; until then each keeps its tier.
- **Types.** A type renamed here publishes its definition under its new name: `ClockParams`,
  `ClockRequest`, `ClockState`, `BuildStatusParams`, `ScreenshotParams`, `Screenshot`,
  `Windows`, `EventName`, `ScrollUntilVisibleParams`. `Windows` is the output of `window.list`,
  so no two public items are `WindowList`; `app.build` answers the hello's `AppDescription`, so
  no output is named as its marker is.

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
        // The method folded into `window.screenshot`.
        "/methods=capture.rendered",
        // Definitions and top-level types published under the names their types had.
        "/$defs/MotionParams", "/$defs/MotionRequest", "/$defs/MotionState",
        "/MotionRequest", "/MotionState",
        "/$defs/BuildReportParams", "/$defs/CaptureParams", "/$defs/CaptureMeta",
        "/$defs/WindowList", "/$defs/EventKind", "/$defs/ScrollUntilParams",
        // The eight event names of §5, each a variant of `Event` tagged by `event`.
        "/$defs/Event/oneOf/event=frame", "/$defs/Event/oneOf/event=input",
        "/$defs/Event/oneOf/event=semantics", "/$defs/Event/oneOf/event=window",
        "/$defs/Event/oneOf/event=announcement", "/$defs/Event/oneOf/event=gap",
        "/$defs/Event/oneOf/event=record", "/$defs/Event/oneOf/event=record_native",
        // Fields.
        "/$defs/EventFilter/properties/kind",
        "/$defs/WindowEvent/properties/opened",
        "/$defs/Matches/properties/truncated",
        "/$defs/Placement/properties/platform",
    ],
    required: &["/$defs/EventFilter/required/event"],
}
```

Everything the entry lists is in the change that makes version 0.33: the renames, the events,
`Matches::omitted`, `window.environment` and the field `Placement` loses to it. `app.build` and
`app.config` add a method and optional fields and may follow in a later, additive version.

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
| `record.start`, `record.stop` | `recording.start`, `recording.stop` | FLUI's own noun: ADR-0215's input recording, the output type `Recording`, the handle kind `recording`, `flui record`. Playwright MCP's `browser_start_recording` (`README.md:1494`) is the same thing, the user's actions recorded as code; Maestro's `startRecording` (`YamlFluentCommand.kt:144`) is video. A trace (ADR-0241) is the driver's file of a session and may hold a recording |
| `run.reload` | `app.reload` | the Dart MCP server's `hot_reload` (`README.md:214`); BiDi `browsingContext.reload` (`index.bs:5156`) |
| `semantics.act` | `element.perform_action` | Detox `performAccessibilityAction` (`docs/api/actions.md:29`); Compose `performSemanticsAction` (<https://developer.android.com/reference/kotlin/androidx/compose/ui/test/SemanticsNodeInteraction>). Their qualifier is left out: `semantics` is the word the group dropped, and the group `element` already says what the action is on |
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
- **Errors.** `ErrorCode` and the error object stay as they are. A code names the state that
  refused, never the method. A new method uses a code the list has before it adds one; a new
  code is added by the record that needs it. `not_supported` names the missing `capability`.
- **Versions.** The break is one version. An app that speaks it refuses an older client
  `version`, naming both (`ProtocolVersion::serves`). No server accepts an old name. A trace's
  header names the protocol it was recorded in (ADR-0241 §2): beside the `flui_trace` check of
  ADR-0241 §5, the trace reader refuses a trace whose protocol a break of `BREAKING` separates
  from its own, with the same error, and the golden trace is recorded again.

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

| Was | Is |
|-----|----|
| `frame` | `window.frame` |
| `input` | `input.entry_added` |
| `semantics` | `window.snapshot_changed` |
| `window`, with `opened: bool` | `window.opened`, `window.closed` |
| `announcement` | `window.announcement_added` |
| `gap` | `events.gap` |
| `record` | `recording.row` |
| `record_native` | `recording.native_line` |

- An occurrence is a subject and a past participle (`opened`, `snapshot_changed`), as BiDi's
  `browsingContext.contextCreated` (`index.bs:5798`).
- A journal's event is `<entry>_added`, as BiDi's `log.entryAdded` (`index.bs:14791`), and
  carries the entry its read returns, with the same `seq`.
- A read a client reads again on change has `<read>_changed`, which carries the revision and
  never the value.
- An item of a stream is the bare noun (`frame`, `row`, `gap`), as BiDi's `script.message`
  (`index.bs:13999`).

Which events an app emits, and what a filter that names one sent unasked selects, are as they
were: a subscription that names `recording.row` selects nothing (ADR-0288 §11), and
`window.announcement_added` is accepted and not yet emitted. Whether such a filter should be
refused is a question of delivery, left to the record named in "Context".

### §6. Reads of the application's state

Three reads join the table. Each is served from state that exists, at the scope that holds it.
Every type below is `flui-protocol`'s own; where the protocol already publishes a type for the
same fact, the read uses it.

```rust
// flui_protocol::window
/// What the runtime installs above a window's root, as the root holds it. A field with no
/// `Option` answers the root's default where the host observed nothing: it is not evidence
/// that the user chose it.
#[non_exhaustive]
pub struct Environment {
    pub window: WindowId,
    pub platform: Option<Platform>,                 // `session::Platform`
    pub brightness: Brightness,
    pub high_contrast: bool,
    pub motion: MotionPolicy,
    pub text_sizing: TextSizing,
    pub font_weight_adjustment: i32,
    /// BCP 47 tags, preferred first, at most 32; none when the host observed none.
    pub locales: Option<Vec<String>>,
    /// Whether `locales` was cut.
    pub truncated: bool,
    pub regional: Option<Regional>,                 // `native_recording::preferences::Regional`
    pub accent_color: Option<[f32; 4]>,             // as `preferences::Preferences` spells it
    pub scroll_bar_visibility: Option<ScrollBarVisibility>,  // `preferences::`
    pub scroll_track_click: Option<ScrollTrackClick>,        // `preferences::`
    pub view_insets: LogicalInsets,
    pub system_gesture_insets: LogicalInsets,
}

vocabulary! {
    /// The platform's light or dark preference (`MediaQueryData::platform_brightness`). Not
    /// `native_recording::window::Appearance`, which is what a window's frame was given.
    pub enum Brightness { Light => "light", Dark => "dark" }
}
vocabulary! {
    /// The motion policy widgets read (`flui_animation::MotionPolicy`). Not
    /// `preferences::Motion`, which is the host's preference before the app's override.
    pub enum MotionPolicy { Full => "full", Reduce => "reduce" }
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
/// What the application was configured with when it was last composed: each value as
/// configured, not as in effect now.
#[non_exhaustive]
pub struct AppConfigReading {
    pub diagnostics_profile: DiagnosticsProfile,  // development | production
    pub exit_policy: ExitPolicy,                  // on_last_window_closed | explicit_quit
    pub motion_override: MotionOverride,          // follow_system | reduce | full
    pub performance_overlay: bool,
    pub window: WindowDefaults,
    pub storage: StorageReading,
    pub installed: Vec<InstalledHook>,
}
pub struct WindowDefaults {
    pub title: String,
    pub size: LogicalSize,
    pub min_size: Option<LogicalSize>,
    pub max_size: Option<LogicalSize>,
    pub mode: WindowMode,                // `native_recording::window::WindowMode`
    pub resizable: bool,
    pub frame: WindowFrame,              // `native_recording::window::WindowFrame`
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
  runtime keeps one merged value for each field and no record of which were overridden or
  which the host observed, so the read has neither. `Placement` loses `platform` in the same
  version; the size, scale and safe area stay there. `text_sizing` needs a public read of the
  policy's kind on `flui_painting::TextSizing`, whose policy is private today
  (`crates/flui-painting/src/text_painter/sizing.rs:47`, `:67`).
- **`app.build`** answers the hello's `AppDescription`: one type and one source, the value every
  service is made with. The service has one place that stores a description, and both
  `Service::new` and `Service::describe` store through it: it fills `flui` with the service's
  own crate's version and cuts each string to 256 bytes. So every host that serves the agent,
  `flui-app`, the headless kit and a test's own service alike, answers a bounded description
  with no rule of its own, after a `describe` as before it. `flui-app` passes the application's
  identity as `name`, where it passes the window title today. `version` is absent until
  `AppIdentity` can carry one.
- **`app.config`** is answered where a host handed a reading and refused `not_supported`,
  naming `app.config`, where it handed none; the hello then does not list the method. Only a
  host that composed an `AppConfig` has one to hand: `flui-app`. The headless kit's and a
  test's services pass `None` and claim no configuration. What leaves the app is held by the
  reading's types: `StorageDir` has no path, `InstalledHook` has no payload, and the place that
  stores the reading cuts the title to 256 bytes. `flui-app` builds the reading by taking
  `AppConfig` apart field by field, with no `..`, and names each hook in a match with no `_`
  arm, so a field or a hook added to `AppConfig` does not compile until the reading states it
  or says why it is left out. Left out today, for want of a wire type: `position`,
  `os_options`, `frame_latency`, `input_latency`, `pointer_resampling`, `surface_color`,
  `frame_failure_detail`, `executors`, `services` and `memory_reclaimers`.
- **When the readings change.** They are replaced whole each time the application is composed:
  at launch, and at a Restart reload, which keeps the process and composes the application
  again from its entry (ADR-0393). Launch is the only composition built today. A value the app
  changes while it runs (`set_motion_override`, `set_performance_overlay`) is read where it
  lives, and `app.config` keeps what was configured.
- **Text.** `title` and `name` are fields the trace writer already redacts by name
  (`TEXT_FIELDS`, `crates/flui-driver/src/trace/redact.rs:21`); each new string field is put on
  one side by `every_text_field_of_the_wire_is_redacted_or_named_as_kept`.
- **There is no `app.theme`.** A theme is a value a package publishes down a subtree, which the
  runtime cannot name and which differs by element. Its home is `inspect.inherited`, the
  inherited values in effect at an element as inspected nodes, which needs a walk `flui-view`
  does not expose and an `Inspect` description the theme types do not have.
- **Overrides are `emulation`'s, when it is built.** The runtime has one cell for the motion
  override, which the application also writes
  (`crates/flui-runtime/src/ui_runtime/mod.rs:188`), so a client's override and its end need a
  layer the runtime does not have. That, and a read of what an override set, are decided with
  `emulation`.

A read added later is a window method served in `agent_serve`, or a value a host hands the
service. It adds no call from the runtime's own paths (ADR-0402 §1), and its row and type state
its scope and its bound.

### §7. An MCP tool is named for its method

An MCP tool's name is its method's wire name with the dot as `_`: `window_snapshot`,
`element_tap`, `app_reload`. A group has no underscore, so the name splits back into its method
at the first one. The tool list is made from the method table (ADR-0238 §1), so no tool has a
name of its own, and the seven hand-written names go when that list is generated.

```rust
impl MethodRow {
    /// `group_member`: the wire name with its dot as `_`.
    pub fn tool_name(&self) -> String;
}
```

Until the list is generated the seven shipped tools keep their `flui_*` names and call the
methods under their new names. Everything else about the adapter stays as ADR-0238 §9 and the
shipped server have it until the record named in "Context" decides it.

### Consequences

- Good, because a later method is placed by a stated procedure into a closed table, and the
  rows the driver spec plans each have a group before they are built.
- Good, because a model's tool list will read as the wire does, and the CLI, the kit and a
  trace spell a method one way.
- Good, because the reads of §6 cost the runtime no state and a release binary no code.
- Bad, because 19 names, 8 event names, 5 `Tester` verbs and a tier's name change at once:
  several hundred mentions across `crates/` and `docs/`, most of them in `flui-protocol`,
  `flui-cli`, `flui-runtime` and `flui-driver`, the schema's next golden and the golden trace.
- Bad, because an app that speaks 0.33 refuses every older client, and a `flui` tool older than
  the app it finds says `version` and nothing more helpful.
- Bad, because the table test tells an examination's row and an overlay's from the rest, and a
  lane's when one exists, but not an instrument's row from a subject's: that placement is the
  procedure's, read by a person, until a row can say what it leaves in the app.
- Bad, because `element.perform_action` and `input.perform_actions` differ by a letter and a
  group. They are in different tiers, and each description opens with the difference.
- Bad, because the theme, the resolved locale and the text direction stay unread until
  `inspect.inherited` is built, and `window.environment` cannot say which values the host
  observed.
- Bad, because the test of the kit's verbs sees the verbs its table lists: a verb added to
  `Tester` and left out of the table is not seen by it.
- Neutral, because `flui run --json` keeps its own event names (`run.reload`, `build.phase`):
  that stream is the command's, not the protocol's.

### Confirmation

Two changes build this record: the names, events, conventions and `window.environment` with
protocol 0.33, and `app.build` and `app.config` in that version or a later additive one.
`docs/gaps.md` has a bullet for §1 to §5, one for §6 and one for §7.

| Rule | Held by |
|------|---------|
| A name is `group.member` in lower snake case, a group has no underscore, the table is sorted, and a tool name splits back into its method | `flui-protocol`'s `every_method_is_named_group_dot_member_once_and_listed_in_order`, which today's `every_method_is_named_group_dot_verb_once_and_listed_in_order` becomes |
| Every row's group is in `GROUPS`; an examination group's rows are read-only and share one output type, and a row with that output type is in that group; an annotating row is in `overlay` and `overlay` has no other; a lane's row is in a lane group and a lane group has no other | `every_row_fits_the_kind_of_its_group`, over `for_each_method!` with each row's output type |
| The rows that change the app are exactly a list | `only_these_rows_change_the_app`, which replaces `what_ends_the_app_is_destructive_and_reads_never_act`: the acting and destructive rows by name, and the destructive ones `app.reload` and `session.close`, so a read declared to act is a line of the diff |
| A name `BREAKING` respelled is a method under the last name its chain reaches, and under no earlier one | `every_respelled_method_is_a_method_under_its_latest_name`, over every entry of `BREAKING`. It replaces the assertions of `the_ops_protocol_0_5_renamed_are_methods_now` and `protocol_0_5_renames_every_0_4_operation_into_the_method_table` that `windows.list`, `semantics.read`, `semantics.act` and `motion.set` are methods; the second test keeps its check that every 0.4 operation is listed |
| The schema is 0.33's golden, and differs from 0.32's only by what `BREAKING` lists | `the_wire_schema_is_the_one_its_version_published`, `every_published_schema_is_additive_to_the_next` |
| What a break lists as removed or respelled is gone from its version's schema | `nothing_a_break_removed_is_in_its_schema`, over every entry of `BREAKING` and its golden: a listed path or old name still present fails, so a field left in place is not passed by the entry that permits its removal |
| Every wire name is lower snake case | `every_wire_name_is_lower_snake_case`, over the schema's property names and enumerated strings; `every_vocabulary_enum_is_spelled_as_it_is_named` |
| A duration or a time names its unit | `a_duration_or_a_time_names_its_unit`, over the schema: a numeric property named for a time word ends `_ms`, `_us` or `_ns`, but for `t` and `arrival`. It sees a field by its name, so a time under another word is not seen |
| A read states its bound | the compiler: a trait in the test file, `BoundedRead`, implemented for each read-only marker by setting its typed `max_…` field or declaring it fixed-size; `every_read_names_its_bound_or_is_fixed_size` walks `for_each_method!` and does not build for a read without the impl or an impl that names a field its parameters lack |
| A journal is read by `since` and answers `entries` and `dropped` | `every_journal_is_read_since_a_sequence_number`, over `input.journal` and `window.announcements` |
| Every event name is `group.name` with a group of `GROUPS` | `every_event_is_named_for_a_group_of_the_table` |
| The core set and its bound | `the_core_set_is_the_thirteen_tools_mcp_lists_by_default`, with the new names |
| `window.screenshot` answers the presented frame when no source is named, the rendered one where the surface cannot be copied, and a named source or `not_supported` | the test of the first host that answers a source, `a_screenshot_answers_the_source_it_names_or_refuses`; until one does, `flui-runtime`'s `window_answers` lists the row as unanswered and `docs/gaps.md` owes it |
| A method verb of `Tester` is named for its row, and every window method has its verb or the reason it has none | `flui-testing`'s `every_method_verb_is_its_methods_member`: a table made by a macro from each verb's own call, over a connection that records the methods sent. It fails a listed verb that sends another row or carries another name, and a window method with no entry |
| `window.environment` answers the root's values, and `text_sizing` its policy | `flui-runtime`'s `a_windows_environment_is_what_its_root_media_query_holds`, with a fixed, a linear and an exact policy |
| Every service answers `app.build` bounded and with FLUI's version, after a `describe` as after `new`; one handed no reading refuses `app.config` and does not list it | `flui-runtime`'s `a_description_is_bounded_and_versioned_however_it_was_stored`, `a_service_refuses_a_configuration_it_was_not_given`; `flui-testing`'s `the_headless_service_claims_no_configuration` |
| The hello names the application, not its window | `flui-app`'s `the_hello_names_the_application_not_its_window` |
| `app.config` states every field of `AppConfig` or leaves it out by name, sends no path, and a hook as its name alone | the compiler, over the reading's builder in `flui-app` (no `..`, no `_` arm); the types `StorageDir` and `InstalledHook`; `flui-app`'s `a_configured_storage_directory_is_answered_as_custom` |
| A new string field is redacted in a trace or named as kept | `flui-driver`'s `every_text_field_of_the_wire_is_redacted_or_named_as_kept` |
| A trace from before a break is refused | `flui-driver`'s `a_trace_from_before_a_break_is_refused_with_both_versions` |
| A read links nothing into a release binary | the `size` job, and the reader of a change to `ui_runtime` (ADR-0402) |

No name is left under an alias. After the change that makes 0.33, `rg` of each old method name
of §3's table, each old event name of §5's and each old `Tester` verb, across `crates/`,
`packages/`, `src/`, `tests/`, `examples/`, `tools/`, `.github/` and `docs/`, finds them only
in `docs/adr/`, in `crates/flui-protocol/src/version.rs`, under
`crates/flui-protocol/tests/schema/`, and in the fixtures of
`crates/flui-protocol/tests/wire_schema.rs` that stand for an older schema. `changelog.d/` and
`CHANGELOG.md` say what past changes did and are not searched.

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
- Good, because a new group is a decision with a source, not the choice of the change that
  needed a name.
- Bad, because a group cannot be made by the change that needs it: it takes a record.
- Bad, because the test holds two of the four kinds by a row's own columns, and the other two
  by the table alone.

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

### 7. Reads of what the runtime and the service hold

- Good, because nothing is invented and each read says where its value lives.
- Bad, because what lives in a package's subtree (a theme) waits for a generic read.

### 8. A registry packages fill

- Good, because `app.theme` would exist.
- Bad, because a theme is not the app's: two subtrees can hold two, and the registry would
  answer one of them.
- Bad, because it is new process-wide state every package must remember to fill, a copy that
  can miss its source.

### 9. The method's name with its dot as `_`

- Good, because it is one name: a model that reads a trace, a test or the CLI's help knows the
  tool.
- Good, because it is the spelling every server read uses (Playwright MCP's `browser_snapshot`,
  `README.md:1155`; Chrome DevTools MCP's `take_snapshot`, `tool-reference.md:43`).
- Bad, because the name is group first where those servers are verb first.

### 10. The wire name unchanged

- Good, because nothing is mapped: MCP allows a dot in a tool name
  (`docs/specification/2026-07-28/server/tools.mdx`, "Tool Names").
- Bad, because no server read uses a dot, and whether every client and model API accepts one
  was not established.

### 11. Names written for the tool list

- Good, because each can be worded for a model.
- Bad, because it is a second name for each method, and the seven shipped ones already differ
  from the methods they call.

## More information

- The questions this record leaves to the next are listed in the driver spec's CLI and MCP task
  (`docs/plans/specs/flui-driver/tasks.md`).
- Revisit `wait.settled` and `wait.frames`, which are `wait.until` with a fixed condition, when
  the kit no longer calls them by name.
