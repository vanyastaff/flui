# ADR-0288: The development agent is one sans-IO service; transports only move lines

- **Status:** Implemented (2026-11-03)
- **Date:** 2026-11-03
- **Supersedes in part:** [ADR-0239](ADR-0239-every-debug-app-hosts-the-driver-agent.md) §1,
  where the server lives: the conversation is `flui-runtime`'s, and `flui-app` keeps the desktop
  transport and the launch; and §6's endpoint thread that "waits a bounded time": nothing waits,
  a session is polled. Unchanged: the build rule, the launch, the endpoints and their
  credentials, owner-inbox execution and `may_have_run` on a timed-out act.
- **Amends:** [ADR-0095](ADR-0095-agent-protocol-schema-crate.md) §3, the seam: an
  `AgentAnswer`, an `AgentEvents` and an `AgentRecording` tell a waker when they can be read; and
  [ADR-0238](ADR-0238-flui-driver-protocol.md) §3, the error codes: `not_driver` (§8).
- **Related:** [ADR-0238](ADR-0238-flui-driver-protocol.md) (the protocol);
  [ADR-0287](ADR-0287-owner-state-is-committed-before-user-code-runs.md) (user code runs after
  the state is committed); [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md) (what
  `flui-runtime` may depend on); [ADR-0215](ADR-0215-input-recording-and-backend-conformance.md) (the recording
  format and its taps, §11)

## Context and problem statement

The driver agent's server lived in `flui-app` (`app::agent::served`): the hello, the token, the
queue cap, the subscriptions, the dispatch over the protocol's table and the window registry,
all private to the desktop transport. Each connection had a thread that polled its socket every
10 ms (with a 200 µs spin while bytes moved) and a worker thread that blocked on each answer with
`recv_timeout`. Three things followed:

| Problem | Where it shows |
|---|---|
| The conversation cannot be reused | a headless run (`flui-testing`, the driver's own suite) would copy the server to serve the same protocol |
| It cannot run on the web | the page has no thread to block on an answer or to sleep between polls |
| It costs latency and wakes | every answer waits for the next poll; an idle connection wakes 100 times a second |

How does every host serve the same conversation, with the same limits, without a thread per
request and without polling?

## Decision drivers

- One conversation for every transport: a limit, a refusal or an order holds on each of them, and
  is written once (AGENTS.md, "Before each push" §7).
- The frame path is synchronous: no `async` in build, layout or paint, and no `Future` that
  invites `.await` on it (AGENTS.md).
- A waker is user code: it runs with no borrow or lock held, after the value it announces is in
  place (ADR-0287), and the agent's failure never fails the app it serves (ADR-0239).
- An app that never starts an agent pays nothing at run time: no thread, no socket, no
  allocation.
- `flui-runtime` has no host, platform or GPU edge (ADR-0083).

## Considered options

1. Keep the server in `flui-app`, a thread per connection and a worker per request.
2. A sans-IO service in `flui-runtime`: a session takes bytes and instants, hands back lines and a
   deadline, and the seam's answers wake a waker; each transport drives it.
3. An `async` service: `impl Future for AgentAnswer`, a session as an `async fn`, an executor
   per transport.
4. The sans-IO service in `flui-view`, next to the seam.

## Decision

Chosen option: 2, because the conversation becomes a value a transport drives, so the desktop
socket, the page's event loop and a test's pump run the same code, and nothing in it blocks.

§1. **One service, always compiled.** `flui_runtime::agent` holds the conversation:
`Service` (the windows the host handed it, the token, `Limits`, the app's introduction, the
driver's claim) and `Session` (one conversation). It is compiled in every build, with no
feature: a feature of `flui-runtime` could not follow `debug_assertions`, so every `flui-app`
build would turn it on anyway. An app that never starts an agent creates no `Service`, so it pays
for the code only.

§2. **A session does no I/O.** `Session` never reads, writes, sleeps, spawns or reads a clock. A
transport drives it:

```rust
let mut session = service.accept(now)?;      // Refused::Full | Refused::Stopped
session.receive(&bytes, now);                // what the connection read
session.peer_closed();                       // its input ended
session.poll(now, &waker);                   // run what it holds; waker woken when more
while let Some(line) = session.transmit() { /* write it */ }
session.next_deadline();                     // poll again by then, woken or not
session.status();                            // Open | Closing (linger) | Closed
```

Framing (`LineReader`, `MAX_LINE`), the hello, the token compare, the version check, refusals,
lingering after a refusal, the queue cap, timeouts, subscriptions and the dispatch are the
session's. Dropping a session ends the conversation and lets go of the requests it holds.

§3. **Completion, not blocking.** `AgentAnswer::register(&Waker)` and
`AgentEvents::register(&Waker)` keep one waker each (the last registered, woken once). A reply
wakes it after its answer is in the channel, and when its last clone drops unsent (the owner
went); an event queue wakes it after the event is queued, once the hub's borrow is released. A
waker is cloned, woken and dropped with no lock held. Its panic is contained and traced, never
resumed into the owner that woke it: what it announces is already in place, and a failure of the
agent's transport must not fail the frame of the app it serves, so ADR-0287's first-failure rule
stops at this seam. There is no `impl Future`.

§4. **Order and bounds.** Requests run in order, one at a time, so an act followed by a read reads
the act's effect. A conversation holds at most `Limits::queue` requests, waiting, running and
answered but not yet transmitted; one more is answered `busy`. A request answers `timeout` at
`reply_timeout` plus what the window may hold it for (a wait's timeout): retry `soon` when it
only reads, `may_have_run` when it acts. Events are taken between answers, in turn from each
stream, while fewer than 64 lines wait to be transmitted; a stream not read keeps its events and
counts the ones it has no room for. More than 1024 lines waiting means the client is not reading,
and the conversation closes. A window too busy to give a subscription its stream is asked again
20 ms later.

§5. **The driver.** A service given an `on_driver_gone` obligation makes the first conversation
past its hello the app's driver. When that session is dropped while the service is not stopped,
the obligation runs once, contained, its panic traced.

§6. **Transports only move bytes.** The desktop transport (`flui-app`) binds the endpoint and
accepts in a blocking call that a stop wakes by connecting to it. Each connection's thread parks
until the session's waker or deadline wakes it:

- Unix: a reader thread blocks on the socket and hands each read over; a stop shuts the socket
  down, which ends its blocking reads and writes; a write blocked for 5 s drops the connection.
- Windows: a pipe takes no timeout, and a blocking read on it cannot be woken without
  discarding what the client has not read yet. The connection's thread reads without blocking
  and looks for input every 10 ms while nothing else wakes it; answers and events wake it at once.

Android is the Unix transport over an abstract socket. On Linux and Android an address `@name`
names the abstract socket `name`, the only kind Android's driver reaches, through
`adb forward … localabstract:name`. An abstract socket has no file, so no directory keeps it the
user's, and any process of the system reaches it; on Android any other app could also start this
one with an endpoint and a token of its choosing. So each accepted connection's peer is read
(`SO_PEERCRED`) before anything else, and one whose effective user is not the app's own, root, or
on Android the shell (`adb forward` connects as it) is closed unread. A headless run drives the
same `Session` over its own pump.

The web has no thread and no socket: the page's event loop is the transport. The launch names a
global (`__flui_agent`), and `flui-native`'s `PageExport` defines it as
`{ version, connect(onLine) }`; `connect` returns `{ version, send(line), close() }`, both objects
carrying the shape's version (`PAGE_EXPORT_VERSION`, 1) so the shape can grow without breaking a
driver that reads it (`flui_protocol::launch::PAGE_EXPORT_VERSION`, which `flui-app` checks
against `flui-native`'s at compile time). Every line the app writes reaches `onLine` as a string,
`onLine(null)` tells the script the app ended the connection, and a driver reaches it through the
browser's own remote protocol: `flui run --agent` speaks WebDriver BiDi to Firefox (a channel as
`onLine`, `send` called through `script.callFunction`) and the DevTools protocol to the Chromium
family (a binding, `Runtime.callFunctionOn`), neither needing a driver of its own, behind one
`BrowserLink` of `flui-cli`'s; Safari has neither (`docs/gaps.md` entry 42). The command serves
the session's usual local endpoint and carries each connection to the page, so `flui mcp` does
not change. A connection the service does not accept (one past
`Limits::max_connections`, or any once it stopped) makes `connect` throw the refusal; a second
driver is refused by its hello, as on every transport (§8). Each connection's conversation is a
task of the page's executor: a line the script sends is handed to the session at once (a line
past `MAX_LINE` is copied no further than `MAX_LINE + 1` units, so the session refuses it as on a
socket), the task polls the session when its waker is woken, the session's deadline is kept with
the page's timer (`PageTimer`), and the lines are written to `onLine` with nothing borrowed. An
`onLine` that throws, or a deadline no timer can keep, ends the connection. Every JS binding stays
in `flui-native`; `flui-app` drives the service through `PageExport`'s Rust traits. The export is
not `Send`, so a task of the page's executor holds it while the hook holds a handle that may cross
a thread: stopping stops the service at once, and at the page's next turn the global goes and
every connection ends.

§7. **The loop owns the hooks.** `DevAgentHost` is a value the event loop owns, with no
`Arc<Mutex>`: `DevAgentHost::attach(hooks)` attaches the hooks the loop took from its
configuration, every call that runs a hook takes `&mut self`, so a hook cannot reach the host and
nothing re-enters it, and dropping the host detaches each hook, then drops it, each contained. A
configuration holds each hook until the first loop that starts with it takes it. `flui-app` keeps
the host in the loop's state and lends it out for each call, so no borrow of that state is held
while a hook runs, and drops it as the loop ends, before the windows it serves are retired; a
loop that ends while the host is lent has the lender drop it. `flui-app`'s
build script names the build that compiles the agent once, `cfg(driver_agent)`, from what Cargo
reports of the profile's debug assertions and the `driver` feature. The launch is read through
one seam, `Platform::launch_value(LaunchKey)`: a `LaunchKey` is a dotted name
(`flui.agent.token`), and each host reads it in its own spelling, the launching intent's string
extra of that name on Android, the environment variable it spells (`FLUI_AGENT_TOKEN`)
elsewhere, an iOS Simulator app's included (`simctl launch` hands it
`SIMCTL_CHILD_FLUI_AGENT_TOKEN` as `FLUI_AGENT_TOKEN`), and the string property of that name of
the page's launch object on the web. The launcher's preload script installs the object as
`globalThis.__FLUI_LAUNCH__` (`PAGE_LAUNCH_GLOBAL`) before the application's module starts; the
web platform takes it as it is made, before the application runs any code: it removes the global
whatever it holds (deleted, or overwritten where a script declared it with `var`), then checks
what it held within bounds (at most 32 properties, each named by a launch key, each a string of
at most 4096 units, checked before it is copied). A malformed object fails the platform's start,
traced, with no value in the message. One key names every spelling, so a runner reads the launch
the same way on every host. The token keeps other origins and other tabs out; a script of the
page's own origin that runs before the module can read it, as a process of the same user can read
a desktop app's environment. Under `EXIT_ON_DISCONNECT` a page's loop quits, which ends its
agents; a page cannot close its tab, so the driver closes it.

§8. **A role in the hello.** The hello names what its client may do: `Hello.role`, a
`ClientRole` of `driver` (the default, which a hello may leave out) or `observer`, in protocol
0.9 and its golden. It sits beside the token and the protocol because it is part of what the
client presents, not a second key of the greeting line. One conversation holds the driver's seat
at a time; a second driver's hello is refused with a new code, `not_driver`, retry `soon`, and
closed. An observer reads and subscribes beside the driver: its hello lists only the methods it
may call, and a call of its that acts is answered `not_driver`. An observer never claims input,
a lease or the exit on disconnect. When the driver's conversation ends the seat is free again,
or, with §5's obligation, the obligation runs and the app is ending, so a later driver's hello
is answered `shutting_down`. This replaces §5's "first conversation past its hello".

§9. **Held input is the connection's.** `input.inject` plays an `ActionSequence` into the
window's own input path, where the platform's input enters (`UiRuntime::handle_input_on`), one
tick per frame boundary, after the frame's commit anchor: a tick lasts as long as its longest
pause or move, on the frame clock, and a move is spread over the frames of its span. Its device
is `DeviceId::AGENT`, reserved but not contactless: its pointers touch down and lift as a
finger's do, each with an id of its own. Its keys go where the platform's do, to the active
window, whichever window the call names. What a sequence leaves held (a contact down, a key
pressed) belongs to the lease it was asked through: an `InputLease` from
`AgentWindow::input_lease`, `Send`, not `Clone`, released when it is dropped. The window takes
`input.inject` and `input.release` through a lease only, and answers them `invalid_argument`
through `AgentWindow::call`. A release cancels the contacts held down and lifts the keys held,
the latest first, and says how many of each. A dropped lease is recorded on the window's port
and released at the owner's next drain, counted before the drain reads its commands: every call
asked through it was enqueued before it was dropped, so the call is taken first, and the work
still queued through the lease is answered `gone`. A `Session` holds one lease per window it
injected into, taken on first use and dropped with the conversation, so a driver that goes
leaves nothing pressed, and two connections never release each other's input. The answer, `ran`
by the `synthetic` path, comes once a frame has passed since the last tick, with the snapshot it
asks for; a sequence that fails part of the way answers the failure with a `partial` effect
counting the ticks that went out. The reply's deadline adds the sequence's span
(`InputParams::holds_for`). A sequence the window cannot play as asked is refused whole before
anything plays: more than 32 sources, 4096 ticks or ten minutes, two sources of one name, a key
that is neither a W3C key name nor the text of one, a point or pressure out of range
(`invalid_argument`), or a trackpad gesture (`not_supported`). A window keeps at most 64 queued
injections and releases, and answers one past that `busy`, retry `soon`.

§10. **A window journals the input it is delivered.** Once an agent has been handed a window,
every input routed to it is journaled where it is routed, the platform's and the agent's on one
path, the agent's marked `injected`; a key is journaled on the active window it goes to. An entry
is journaled before the app handles it, so a handler that panics does not keep its input out,
and goes out at once as an `input` event. Entries take sequence numbers from 1; the journal
keeps the last 4096, and `input.journal` answers the entries after `since`, at most
`max_entries`, oldest first, with `dropped` counting the ones after `since` it let go. A step a
driver plays is an entry: a press, move, release, cancel, scroll or trackpad gesture, a key down
or up, committed text, a composition step. A pointer entering or leaving, a device change,
drag-and-drop and a capability change are not. A pointer the protocol has no class for (one with
no physical contact, or one the platform did not name) is `other`. `input.inject` answers
`observed`, the span of the named window's entries its ticks made.

§11. **A connection records what the runtime applies.** `record.start` names a window and the
layers to record (`RecordLayer`; `input` is the one this build records, and the hello's
`capabilities.record` lists it) and answers `{recording, header}`: the recording's number on the
connection and its first line, in the input recording's format (ADR-0215). The window's
`AgentWindow::record` starts it at the owner's next drain as a tap beside the application's own
(ADR-0215 §1), so it covers every window of that UI runtime: the gesture decisions and
preference revisions are the runtime's, and a replay needs them in order with the input. The
header names the runtime's windows, their scales and gesture values and the preferences in
effect, on the `virtual` clock under a manual clock source and `monotonic` otherwise; its text is
redacted unless `keep_text` asks otherwise (ADR-0215 §5). Each row then goes, to the connection
that started the recording and no other, as a `record` event `{recording, window, row}`, a
recording's line. A subscription that selects `record` selects nothing. The owner queues at most
1024 rows a recording's reader has not taken: past that the rows are counted, and the next row
that fits is preceded by a `gap` row naming how many were lost, so a reader that falls behind
costs the runtime nothing but its own rows. A recording ends with its `end` row, which counts
every row lost: when `record.stop` asks, at the owner's next drain and after the rows admitted
before it; when the window it was started on closes; and when the runtime goes. `record.stop`
answers `{rows, lost_rows, complete}` once the `end` row is written, and a recording that ended
by itself is held, with what it came to, until `record.stop` names it. The connection going
removes the tap with no row. A runtime holds at most 8 recordings and a connection 4
(`Limits::recordings`); one past either is answered `busy`. The recording's number is the
connection's: `record.stop` from another connection is answered `unknown_handle`, kind
`recording`.

### Consequences

- Good, because a headless run, the web runner and the desktop serve one conversation, and a
  limit or a refusal is fixed once.
- Good, because an answer is written as soon as the owner sends it, and an idle Unix connection
  never wakes.
- Good, because `flui-app` no longer depends on `serde` or `serde_json` outside its tests.
- Bad, because `flui-runtime` gains `serde`, `serde_json` and `flui-protocol/serde` as normal
  dependencies, in every build.
- Bad, because a Windows connection still looks for input every 10 ms, and a Windows client that
  stops reading holds its connection until it reads, leaves or the agent stops, since the pipe
  takes no write timeout.
- Bad, because a page's agent stops a turn after it is asked to: the page export is not `Send`,
  so a task of the page's executor holds it, and a stop reaches it at the page's next turn. The
  service refuses new conversations at once.
- Neutral, because the request order, the refusals and their wording are unchanged: the socket
  suite runs as before, faster.

### Confirmation

- `Session`'s API takes bytes and instants and returns lines and deadlines; it names no stream,
  thread or clock (compiler).
- `flui-runtime`'s contract tests (`tests/contracts/agent_service.rs`) drive sessions with no
  thread or socket: `a_conversation_answers_in_order_as_the_owner_drains_and_wakes_its_transport`,
  `an_act_past_its_deadline_answers_timeout_and_still_runs`,
  `a_conversation_without_the_right_hello_in_time_closes_without_a_word`,
  `the_service_takes_conversations_up_to_its_limit_and_none_once_stopped`,
  `the_drivers_end_runs_its_obligation_once_and_not_after_a_stop` and
  `a_subscription_streams_the_windows_events_as_it_publishes_them`. The wake assertions fail with
  either wake removed.
- `flui-app`'s `agent_unlaunched::an_app_launched_without_an_endpoint_starts_no_agent` runs an
  app without an endpoint and finds no `flui-agent-*` thread; it fails when an agent is
  installed regardless.
- `flui-app`'s socket suite (`the_driver_agent_contains_every_failure`) drives the desktop
  transport end to end.
- A hook cannot reach its host: `DevAgentHost` is `!Send` and `!Sync`, and every call that runs a
  hook takes it by `&mut` (compiler). `flui-runtime`'s `dev_agent_host_contains_its_hook` drops a
  hook that panics in any call, alone, keeps the others served in order, and contains a detach or
  a drop that panics while the host goes.
- `flui-app`'s `a_loop_takes_every_installed_hook_once_and_one_that_panics_is_dropped_alone`
  takes three hooks from one configuration, finds a clone of it holding none, and finds the hook
  that panicked in `attach` dropped without a detach;
  `a_host_lent_when_its_loop_ends_is_handed_back_to_be_dropped` ends a loop while its host is
  lent and finds the host dropped, its hook detached once, when it comes back.
- `flui-app`'s build script and the agent's module agree on `cfg(driver_agent)`: a
  `compile_error!` fires in a build where the alias and `any(debug_assertions, feature = "driver")`
  on a system with a local-socket transport, or wasm32, differ.
- `flui-runtime`'s `one_driver_at_a_time_and_an_observer_only_reads` refuses a second driver's
  hello `not_driver` until the first goes, lists an observer only the methods that read, and
  answers its act `not_driver` without running it; `the_drivers_end_runs_its_obligation_once_and_not_after_a_stop`
  runs the obligation for the driver alone and answers a later driver `shutting_down`.
  `flui-protocol`'s `a_hello_names_its_role_only_when_it_observes` pins the spelling, and the
  0.9 golden holds `ClientRole` and `not_driver`.
- The Android transport's socket runs on the Linux host: the socket suite's
  `an_abstract_socket_serves_its_user_and_closes_others_unread` serves a client over `@name` and
  closes unread a peer its rule refuses; `an_abstract_socket_admits_its_user_root_and_on_android_the_shell`
  pins the rule. `flui-native`'s `a_headless_platform_reads_launch_values_from_the_environment`
  and `flui-app`'s `the_launch_is_read_through_its_keys` read the launch through its keys, and
  each key's environment spelling is the variable the protocol names. The Android intent read
  and the abstract socket on a device are compile-checked for `aarch64-linux-android` and run on
  a device by the owner.
- The web transport runs on Node and the Linux host. `flui-native`'s wasm32 suite drives the page
  export against Node's global object:
  `a_page_export_carries_lines_both_ways_and_its_drop_ends_every_connection`,
  `a_page_connection_ends_alone_when_its_script_or_the_application_closes_it` and
  `a_page_timer_runs_once_per_arm_and_never_after_its_cancel_or_drop`; it takes the launch off the
  page with `the_web_platform_takes_its_launch_values_off_the_page_as_it_starts`,
  `a_malformed_page_launch_fails_the_start_and_leaves_the_page_all_the_same` and
  `a_launch_object_a_script_declared_is_cleared_when_it_cannot_be_deleted`, and its Linux tests
  pin the launch object's bounds (`a_malformed_page_launch_is_refused_naming_only_a_launch_key`).
  `flui-app`'s page conversation runs on the Linux host with the test as the page's event loop:
  `a_page_conversation_answers_its_script_and_ends_when_the_script_closes_it`,
  `a_page_conversation_ends_when_its_script_stops_listening_or_no_timer_keeps_its_deadline`,
  `a_page_conversation_keeps_its_session_deadline_through_the_timer`,
  `a_line_sent_while_the_session_runs_closes_the_conversation` and
  `a_stopped_service_ends_its_page_conversations_without_the_driver_going`. `flui-cli`'s
  `chromium_is_driven_over_the_devtools_protocol` and `firefox_is_driven_over_webdriver_bidi`
  start the browser, install the launch and carry a connection to a page standing in for the app,
  through a reload, where the browser is installed; the relay runs against a tab of the test's
  own in `a_client_waits_for_the_page_export_and_is_carried_to_a_connection_of_its_own` and
  `a_page_connection_ends_with_its_client_and_every_one_with_a_reload`. A FLUI app served in a
  browser is run by the owner.
- Held input (§9) is driven through `flui-runtime`'s public API with no thread
  (`tests/contracts/agent_input.rs`):
  `an_injected_tap_reaches_the_app_one_tick_per_frame_and_answers_after_a_frame`,
  `a_pause_holds_the_injection_for_its_duration`,
  `held_input_stays_down_until_the_lease_releases_it`,
  `dropping_the_lease_lets_go_what_it_holds_and_answers_its_queued_work`,
  `a_lease_releases_only_its_own_input`, `input_the_window_cannot_play_as_asked_is_refused_whole`,
  `an_injection_that_fails_part_of_the_way_says_how_far_it_got` and
  `a_closing_window_answers_its_queued_input_gone`; `agent_service.rs`'s
  `input_a_connection_holds_is_let_go_when_it_goes` drops a session holding a contact down and
  finds it cancelled. Each fails with its rule removed: the answer sent at the last tick, the
  release applied before the drain's commands, a call taken without a lease, the queued input
  dropped unanswered at close, and a lease the session never drops.
- The input journal (§10) is pinned in the same file:
  `the_journal_lists_what_the_window_was_delivered_and_an_injection_names_its_entries`,
  `each_journal_entry_goes_out_as_an_input_event` and
  `the_journal_keeps_its_latest_entries_and_counts_the_ones_it_let_go`. Each fails with its rule
  removed: the agent's input not marked, no `input` event, a journal that holds one entry more,
  and an answer without `observed`.
- Recording (§11) is driven through sessions in `agent_service.rs`:
  `a_recording_streams_the_runtimes_rows_to_its_connection_until_it_stops`,
  `a_recording_ends_with_its_window_or_runtime_and_goes_with_its_connection`,
  `a_recording_counts_the_rows_its_reader_fell_behind_on` and
  `a_connection_holds_its_recordings_up_to_its_limit_and_stops_only_its_own`. Each fails with its
  rule removed: the stop never taken at the drain, the `end` row not awaited before the answer,
  the window's close or the runtime's drop leaving the recording open, an unbounded queue, and no
  limit per connection. `flui-protocol`'s `no_two_wire_types_share_a_schema_name` keeps the
  recording's types apart from the session's in the 0.9 golden.

## Pros and cons of the options

### 1. The server in `flui-app`, threads per request (status quo)

- Good, because it works on the desktop and was proven there.
- Bad, because nothing of it serves the web or a headless run, and every answer waits for a poll.

### 2. A sans-IO service in `flui-runtime` (chosen)

- Good, because the conversation is written once and every transport is a few dozen lines.
- Good, because tests drive it with lines and instants, deterministically.
- Bad, because a transport has to honour the driving contract (poll after receive, take every
  read before parking); the desktop one is its reference.

### 3. An `async` service

- Good, because `async` is the usual shape of a network service in Rust.
- Bad, because a `Future` on the seam invites `.await` on the owner thread, where the answer can
  never come.
- Bad, because each transport needs an executor, and hosts without Tokio (embedders, the web
  runner's event loop) would carry one for the agent alone.

### 4. The service in `flui-view`

- Good, because it sits next to `AgentWindow`.
- Bad, because a service that speaks JSON and enforces limits is runtime policy, not the view
  layer's, and `flui-view` would gain `serde_json`.

## Migration

The desktop transport moved in the change that accepted this ADR, the hook host moved into the
loop in the next (§7), and the role came into the hello after it (§8). The Android transport
came next (§6, with the launch read through the platform, §7), and then `flui run --agent` on
Android: the endpoint and the token as `am start` extras, and a session file that names the
loopback port `adb forward` holds instead of the app's own endpoint, which only the device
reaches. The web transport came after it (§6, with the page's launch object, §7), and then
`flui run --agent` in a browser: the browser started with its remote protocol on, the launch
installed by a preload, and the session's endpoint carried to the page. Held input per
connection came after it (§9): a lease the session owns, released when it is dropped. The input
journal came next (§10).
