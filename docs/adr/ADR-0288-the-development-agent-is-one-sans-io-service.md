# ADR-0288: The development agent is one sans-IO service; transports only move lines

- **Status:** Accepted (2026-11-03). Implemented: §§1–8, the desktop and Android transports;
  the web's is to come.
- **Date:** 2026-11-03
- **Supersedes in part:** [ADR-0239](ADR-0239-every-debug-app-hosts-the-driver-agent.md) §1,
  where the server lives: the conversation is `flui-runtime`'s, and `flui-app` keeps the desktop
  transport and the launch; and §6's endpoint thread that "waits a bounded time": nothing waits,
  a session is polled. Unchanged: the build rule, the launch, the endpoints and their
  credentials, owner-inbox execution and `may_have_run` on a timed-out act.
- **Amends:** [ADR-0095](ADR-0095-agent-protocol-schema-crate.md) §3, the seam: an
  `AgentAnswer` and an `AgentEvents` tell a waker when they can be read; and
  [ADR-0238](ADR-0238-flui-driver-protocol.md) §3, the error codes: `not_driver` (§8).
- **Related:** [ADR-0238](ADR-0238-flui-driver-protocol.md) (the protocol);
  [ADR-0287](ADR-0287-owner-state-is-committed-before-user-code-runs.md) (user code runs after
  the state is committed); [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md) (what
  `flui-runtime` may depend on)

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
on Android the shell (`adb forward` connects as it) is closed unread. The web and a headless run
drive the same `Session` over their own transport.

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
`SIMCTL_CHILD_FLUI_AGENT_TOKEN` as `FLUI_AGENT_TOKEN`). The web reads none until its
transport brings the page's preload. One key names both spellings, so a runner reads the launch
the same way on every host.

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
  on a system with a local-socket transport differ.
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
reaches. Then, each in its own change: the web transport (the page's export, driven from the JS
event loop), and held input per connection (a lease the session owns, released when it is
dropped).
