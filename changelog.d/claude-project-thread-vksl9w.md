### Changed

- **A frame that keeps failing backs off.** After a dropped frame (a layout or paint
  panic, a pipeline error) a presentation waits its display's refresh period, doubled for
  each further consecutive drop, up to `flui_runtime::frame_failure::FAILURE_RETRY_CAP`
  (1 s), instead of retrying on every wake. A key press or pointer button press retries at
  once, once per wait.
- **Every debug FLUI app hosts the driver agent** (`flui-app`, ADR-0239): launched with
  `FLUI_AGENT_ENDPOINT` and `FLUI_AGENT_TOKEN`, a debug build (or one with the new `driver`
  feature) serves its windows in protocol 0.9 over a current-user named pipe or a `0700` Unix
  socket, with no application code. `FLUI_AGENT_EXIT_ON_DISCONNECT=1` makes the app exit with
  `flui_protocol::launch::EXIT_DISCONNECTED` when its driver's connection closes.
- **`AppConfig::with_dev_agent` appends** a hook instead of replacing the installed one; each
  hook is contained on its own.
- **`flui run --agent`** needs no feature of the application in a debug build, accepts
  `--release` (building with the `driver` feature of `flui` or `flui-app`, under the key the
  application names it by), and `flui mcp` speaks protocol 0.9.
- **An agent's `wait.until` without `timeout_ms`** waits `WaitParams::DEFAULT_TIMEOUT_MS`
  (5 s) and none waits past `WaitParams::MAX_TIMEOUT_MS` (24 h); the window answers it
  unsatisfied rather than the connection timing out, and a wait whose asker has gone gives
  up its place under the window's cap.
- **`flui_protocol::capture::BlobRef::id`** is a `BlobId`.
- **The driver agent serves every older client** its protocol version does not break
  (`ProtocolVersion::serves`): it greets one with its own version and capabilities, and only
  a newer client, or one across a listed break, is refused `version`. An error that answers no
  request carries no `id` instead of `"id": null`.
- **`flui mcp` reads past events and late answers** on its connection, and treats an error
  that answers no request as its current request's.
- **One typed call per window method** (`flui-view`'s development-agent seam):
  `AgentWindow::call::<M>(params)` takes any `M: flui_protocol::WindowMethod` and answers
  `AgentAnswer<M::Output>`, replacing the per-method `read`, `act`, `act_and_read`, `locate`,
  `actionability`, `wait`, `inspect`, `motion` and `announcements`; `subscribe` stays. Every
  failure is a `flui_protocol::ErrorObject`, and params that address another window are refused
  `invalid_argument`.
- **`semantics.act` by locator resolves and acts in one owner turn**, so the element acted on is
  the one the locator matched on the tree the action starts on; no match answers `not_found`
  and a strict pick that matches several answers `ambiguous` with its candidates.
- **The driver agent advertises only the methods its app serves**: a window method the runtime
  does not answer is left out of `capabilities.methods` and refused `not_supported`.
- **The driver agent's conversation is `flui_runtime::agent`'s** (ADR-0288): one service and
  a session per connection that do no I/O, so every transport serves the same protocol and
  limits. The desktop transport keeps no worker thread and waits on no answer: an owner's answer
  or event is written as soon as it arrives, and an idle Unix connection never wakes.
- **The event loop owns its development-agent hooks** (ADR-0288 §7): the first loop that starts
  with a configuration takes its hooks, so a clone of that configuration starting another loop
  finds none, and the loop detaches and drops them when it ends. A hook is called with nothing
  locked, and one that panics is dropped alone. `flui_runtime::dev_agent::DevAgentHost::attach`
  takes the hooks and its calls take `&mut self`; `HeadlessHost::with_dev_agent` takes the
  `HeadlessDevAgent` by `&mut`.
- **The driver agent admits one driver at a time** (ADR-0288 §8): a second connection whose
  hello asks to drive, as one naming no role does, is refused `not_driver` (retry `soon`) until
  the first goes. Under `FLUI_AGENT_EXIT_ON_DISCONNECT` the app exits with the connection admitted
  as its driver, and a later driver is refused `shutting_down`.
- **The input recording's wire types are `flui_protocol::input_recording`** (ADR-0215 §2):
  `Header`, `Row` and what they nest, plain data a reader reads without the runtime.
  `flui_runtime::{Privacy, WireError}` and `InputRecord::to_wire`/`from_wire` convert to and from
  them; `flui_testing::recording` keeps its API (`RecordingClock` and `Privacy` are re-exports).
- **`InputTaps::install` adds a tap beside the registered ones** instead of replacing the
  registered tap (ADR-0215 §1): each registration is offered the rows admitted after it was
  installed and keeps its own `Gap` and `End` rows and lost-row count, so an application's
  recording and a driver's run side by side. Dropping a handle unregisters its own tap alone.

### Added

- **`HeadlessHost::time_to_next_frame`, `pump_next_frame` and `set_window_display`**: wait
  as a host's loop does before its next frame, and deliver the display a window is on.
- **`LaidOut::dispatch_ime`** delivers an input-method event to the focused field, and the
  testing kit's window now offers a push-model input method, as winit does.
- **`flui_protocol::launch` and `flui_protocol::framing`**: the driver agent's launch contract
  (variable names, Android extras, the shortest token, the exit code) and its framing limits
  and binary-payload header. `Hello::new`, `HelloReply::new`, `AppInfo::new` build the
  handshake.
- **`flui_protocol::envelope`**: the lines of a conversation as types, `Greeting`,
  `Request<P>`, `RequestId` and `ServerLine<R>`, the last read by its keys (`hello`, `event`,
  `error`, `result`) and published in the protocol 0.9 schema. `framing::LineReader` splits a
  byte stream into lines of at most a given length, refusing a longer one with
  `FramingError::LineTooLong`.
- **The facade's `driver` and `devtools` features**: `driver` keeps the agent in a release
  build; `devtools` re-exports `flui-devtools` as `flui::devtools`.
- **`flui_protocol::WindowMethod` and `Addressed`**: the methods of the table marked
  `Agent(window)`, and the window their params address; `for_each_window_method!` walks those
  rows, so a server's dispatch is generated from the one table.
- **`flui_runtime::agent`**: `Service` (the windows a hook serves, `Limits`, the driver's
  obligation) and `Session`, one conversation driven by its transport with `receive`,
  `peer_closed`, `poll`, `transmit`, `next_deadline` and `status`.
- **`AgentAnswer::register` and `AgentEvents::register`** (`flui-view`'s development-agent
  seam): the last waker registered is woken once when the answer or an event can be read, and
  when the owner goes.
- **`flui_protocol::ClientRole` and `Hello::role`**: a hello names `driver` (the default) or
  `observer`; an observer reads and subscribes beside the driver, its hello lists only the
  methods that read, and a call of its that acts is refused with the new
  `ErrorCode::NotDriver`. `flui mcp` says when another client drives the app.
- **The driver agent on Android** (ADR-0288 §6): a debug app started with the intent extras
  `flui.agent.endpoint` (`@name`), `flui.agent.token` and `flui.agent.exit_on_disconnect` serves
  its window over the abstract socket `name`, which a driver reaches through
  `adb forward tcp:0 localabstract:name`. A connection whose peer is not the app's user, root or
  the shell is closed unread. On Linux an endpoint `@name` is an abstract socket that admits
  the app's user and root.
- **`Platform::launch_value(LaunchKey)`** and `SharedPlatform::launch_value` (`flui-native`),
  with `flui_platform::LaunchKey`: the value the process was launched with under a dotted key,
  the launching intent's string extra on Android and the environment variable the key spells
  elsewhere. `flui_native::current_user_id` on Linux and Android.
- **`flui run --agent` on an Android device**: the app is started with the agent's endpoint
  (an abstract socket) and token as `am start` extras, and `flui mcp` reaches it through the
  loopback port of an `adb forward` the run holds and removes when it ends. A release build
  gets the `driver` feature. The session file's `endpoint` is now typed: `{"local": …}` or
  `{"forwarded": "127.0.0.1:<port>"}`, and `run.agent.ready` carries `forwarded` on Android.
- **The driver agent in a page** (ADR-0288 §6): a debug app on the web whose launch object names
  an endpoint serves its window through the global it names: `connect(onLine)` returns
  `{ version, send(line), close() }`, and every line the app writes reaches `onLine`
  (`onLine(null)` when the app ends the connection). A WebDriver BiDi driver connects through
  `script.callFunction` with a channel as `onLine`. Under `flui.agent.exit_on_disconnect` the
  page's loop quits when its driver goes.
- **`flui_native::page_export`** (wasm32): `PageExport`, a versioned global
  (`PAGE_EXPORT_VERSION`) whose script connections reach a `PageExportHandler` as
  `PageConnection`s, with `PageLine` as the application's end; and `PageTimer`, a re-armable
  timeout on the page's event loop.
- **A page's launch values**: the web platform takes the object the launcher's preload installs
  as `globalThis.__FLUI_LAUNCH__` (`flui_platform::PAGE_LAUNCH_GLOBAL`) as it starts, removes
  it, and answers `Platform::launch_value` from its copy; a malformed object fails the start.
  `LaunchKey::is_spelling` checks a key read at run time.
- **`flui run --agent` in a browser** (`--device browser:chrome`, `firefox`, `edge`, `chromium`,
  `brave`, `opera`): the run starts the browser with its remote protocol on and a profile of its
  own, installs the launch in each top-level document before the app's module runs, and carries
  every `flui mcp` connection to the session's usual local endpoint on to a connection of the
  page's; a reload ends them, and the client connects again. Firefox is driven over WebDriver
  BiDi and the Chromium family over the DevTools protocol, with no driver binary; a browser has
  60 s to start and answer the link's first commands. `--headless` runs the browser without a
  window; Safari is refused (`docs/gaps.md` entry 42). `run.agent.ready` carries `relayed` in a
  browser.
- **`flui_protocol::launch::PAGE_LAUNCH_GLOBAL` and `PAGE_EXPORT_VERSION`**: the global a
  driver installs a page's launch under, and the version of the export it connects through.
- **The driver agent injects input** (`input.inject`, `input.release`, ADR-0288 §9): a pointer,
  wheel, keyboard, text or input-method sequence enters the window's own input path one tick per
  frame, and the answer comes once a frame has passed since its last tick. What it leaves held
  is the connection's until `input.release` or the connection goes. Trackpad gestures answer
  `not_supported`.
- **`AgentWindow::input_lease` and `InputLease`** (`flui-view`'s development-agent seam): input
  is injected and released through a lease, `Send` and not `Clone`, whose drop releases what it
  holds.
- **`flui_platform::pointer::DeviceId::AGENT`**: the device of the driver agent's pointers,
  reserved, with contacts.
- **`ActionSequence::ticks` and `ActionSequence::span`** (`flui-protocol`): how many ticks a
  sequence plays and how long it takes; `InputParams::holds_for` is its span, so a server's
  reply deadline covers the input.
- **The driver agent journals a window's input** (`input.journal`, the `input` event,
  ADR-0288 §10): every input the window is delivered, the platform's and the agent's (marked
  `injected`), takes the next sequence number and goes out as an `input` event; the journal keeps
  the last 4096 entries and counts the ones a reader missed. An injection's answer carries
  `observed`, the span of the entries it made.
- **`JournalEntry::new`, `with_position`, `with_injected`, `Journal::new`,
  `JournalParams::new` and `InputEvent::new`** (`flui-protocol`), and
  `DeviceClass::Other`, the class of a pointer with no physical contact or of a kind the
  platform did not name.
- **The driver agent records** (`record.start`, `record.stop`, the `record` event, ADR-0288
  §11): a connection records what a window's UI runtime applies in the input recording's format,
  its header in the answer and each row sent to it alone as a `record` event; a reader that
  falls behind gets a `gap` row, and the recording ends with its `end` row at `record.stop`,
  when its window closes or when the runtime goes. The hello lists the layers it records in
  `capabilities.record`; `Limits::recordings` bounds a connection's.
- **`AgentWindow::record` and `AgentRecording`** (`flui-view`'s development-agent seam): the
  recording's header, its rows, a waker, and `stop`; dropping it ends the recording with no
  row. `flui_protocol::{RecordParams, Recording, RecordStopParams, RecordSummary, RecordEvent,
  RecordLayer}`, `HandleKind::Recording`, `RecordingStart::to_wire` (`flui-runtime`) and
  `HeadlessHost::serve_window`, which hands a window it opened to a development agent.
- **`flui record <file>`** writes a running app's input recording (the rows `flui-testing`
  replays) to a file or, with `-`, to stdout, until Ctrl-C or until the window closes.
  `--window` names the window and `--keep-text` keeps the rows' text. It connects as an
  observer, so it records beside a `flui mcp` that drives the app.

### Removed

- **`flui-devtools`' `agent` feature and `AgentServer`**, and the facade's `dev-agent` feature:
  the driver agent is `flui-app`'s. `flui create` templates no longer install a hook.
- **`flui_runtime::ui_runtime::SemanticsAgent`, `AgentReply`, `AgentError` and
  `UiRuntime::semantics_agent`**: an agent reaches a window through
  `UiRuntime::dev_agent_window`. `flui_view::dev_agent::{AgentFault, HandleKind,
  InspectTarget}` go with the per-method calls.
- **`flui_runtime::dev_agent::DevAgentAttachment`, `DevAgentHost::new` and
  `DevAgentHost::is_attached`**: `DevAgentHost::attach(hooks)` builds a host that serves until it
  is dropped, and `DevAgentHost::is_serving` says whether any of its hooks serves.
