# ADR-0327: The native host is owner-local and runs its IO on the runtime its creator hands it

- **Status:** Accepted (2026-11-12)
- **Date:** 2026-11-12
- **Supersedes:** [ADR-0151](ADR-0151-platform-layer-boundary-and-names.md) §8 only (the clause
  that makes `tokio::runtime::Handle` `pub(crate)` in the core host: `NativeExecutor::from_tokio`
  takes one); [ADR-0082](ADR-0082-platform-api-contract-crate.md) §4 only (an owner-only
  callback is kept sound by `OwnerSlot`'s run-time refusal: the compiler keeps it sound once the
  window is not `Send`); [ADR-0039](ADR-0039-event-loop-affinity-capability.md) §1's `shared()` and §2 only (which
  methods stay on a `Send + Sync` `Platform`; another thread reaches the owner through
  `PlatformProxy` alone)
- **Related:** [ADR-0323](ADR-0323-a-value-stays-on-the-owner-thread-unless-its-signature-names-what-crosses.md)
  §1, §2 and §4 (the rule this record applies to `flui-native`, and the five capability traits
  of `flui-platform` whose bounds wait on it), [ADR-0047](ADR-0047-unified-execution-services.md)
  (one set of execution pools per process), [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md)
  (`flui-runtime` has no host edge), [ADR-0266](ADR-0266-one-owner-thread-ui-runtimes-isolate.md) §1 (one
  owner thread per process, whose loop this record makes owner-local), [ADR-0303](ADR-0303-a-rule-takes-the-first-rung-that-states-it-and-an-xtask-gate-is-the-last.md)
  (a type before a run-time check), [ADR-0258](ADR-0258-owner-turn-panic-boundary-and-terminal-exit.md) §3 and §7
  (the exit order and deadline a source must not hold)

## Context and problem statement

ADR-0323 §4 takes `Send + Sync` off `DragTransport`, `PlatformDragSource`, `PlatformHaptics`,
`PlatformSystemChrome` and `PlatformTextInput`. Removing them, the compiler names a thread at one
place only: `flui-native`'s storage, which keeps every window facet inside thread-safe
containers although only the owner thread reaches it. The same crate starts a tokio runtime of its
own beside the one `flui-runtime`'s execution services start (ADR-0082 §5 left that for "ADR-0047's
consolidation"). The measured state on `main` at `8b2b210`:

| Fact | Evidence |
|---|---|
| The loop and its window state are `Send + Sync` | `pub trait Loop: Send + Sync` with `type Shared: Send + Sync` (`crates/flui-native/src/host.rs:137-138`), `OpenWindows: Send + Sync` (`owner/handlers.rs`), `HeadlessWindows { tracked: Mutex<Tracked> }` holding `WindowFacets` (`window/headless.rs`) |
| The platform trait is `Send + Sync` and checked by debug assertions | `pub trait Platform: sealed::Sealed + Send + Sync` (`platform.rs:155`), `OwnerAffinity` (ADR-0039, "Implementation status"), `SharedPlatform { platform: Arc<dyn Platform> }` (`owner/mod.rs:367`) |
| An owner-only value inside a `Send` window is refused at run time | `OwnerSlot`/`OwnerCell`/`OwnerMap` (`owner/slot.rs`), `an_input_callback_registered_off_the_owner_thread_is_refused` |
| `unsafe impl Send/Sync` exist only to satisfy the loop's bound | `MacOSLoop` (`owner/apple/macos.rs`), `WebLoop` (`owner/web.rs`) |
| A host owner starts a 2-thread tokio runtime of its own | `BackgroundExecutor` (`executor.rs`), created at `owner/linux/x11.rs`, `owner/linux/wayland.rs`, `owner/windows.rs`; its users are the IBus/XIM bus, `logind::watch`, the settings portal and one Wayland `spawn_blocking` |
| The execution services start their IO pool too late for the host | `install_host_executors` and `ensure_execution` run inside `ready` (`crates/flui-app/src/app/runner/main_window.rs`), after `X11Loop::try_new` started the input bus |

## Decision drivers

- The compiler proves which thread touches a value (ADR-0323 §1); a run-time refusal or a debug
  assertion is the lower rung (ADR-0303).
- One tokio runtime per process for IO (ADR-0047), and `flui-native` gains no edge to
  `flui-runtime` (ADR-0083, ADR-0081).
- XIM, logind and D-Bus need tokio's IO and time drivers on the Linux hosts; that does not change.
- `flui-native` still runs without an application: its own tests, `flui-testing`'s dev
  dependencies, headless.
- Exit stays bounded by ADR-0258 §7, and a source that hangs does not hold it.

## Considered options

1. The loop and windows are owner-local; the host takes a tokio runtime handle from its creator.
2. The same, with the host taking `Arc<dyn flui_platform::executor::Executor>` and checking for a
   tokio context at run time.
3. Keep the thread-safe storage and put `Send + Sync` back on the five traits as kind `handle`.

## Decision

Chosen option: 1.

### §1. What is owner-local

`Rc` with `Cell`/`RefCell`, no `Mutex`, no `Send` or `Sync`:

```rust
pub trait Loop: Sized + 'static { type Shared: 'static; /* … */ }
fn run(self: Rc<Self>, platform: Rc<dyn Platform>, handler: Rc<dyn AppHandler>);
pub trait Platform: sealed::Sealed + 'static { /* … */ }
pub(crate) struct WindowFacets {
    haptics: Rc<dyn PlatformHaptics>, chrome: Rc<dyn PlatformSystemChrome>,
    text_input: Rc<dyn TextInput>, accessibility: Rc<dyn WindowAccessibility>, /* … */
}
pub(crate) struct HeadlessWindows { tracked: RefCell<Tracked> }
impl<H: Host> HostPlatform<H> { pub fn run_shared(self: Rc<Self>, handler: Rc<dyn AppHandler>) -> Result<(), PlatformError>; }
pub fn on_wake(&self, turn: Box<dyn FnMut()>); // OwnerPlatform
```

`OpenWindows`, `Window::Shared`, `OwnerHooks` and each host's loop follow. `exit_reevaluation`
is a `Cell<bool>`. The `unsafe impl Send/Sync` of `MacOSLoop` and `WebLoop` are deleted.
`OwnerSlot`, `OwnerCell` and `OwnerMap` are deleted with their last user: a callback registered on
a window is reached only through an `Rc` window, which only the owner holds, so the refusal becomes
a compile error. `data_transfer`, `data_store` and `release_session_end` move to `OwnerPlatform`,
where every caller already is.

One owner thread runs one loop (ADR-0266 §1), and its registries are its own `Rc`s.

### §2. What stays a handle

Values another thread reaches keep `Send + Sync`, each with its ADR-0323 §2 kind:

| Value | Who reaches it | Kind |
|---|---|---|
| `OwnerSignal`: the `WindowRequest` queue, `quit`, the exit-policy request, window alarms | worker threads and UI runtimes through `PlatformProxy`, the driver agent's thread, IO tasks that wake the owner | handle |
| `PlatformProxy`, `ProxyTransport`, `PendingWindow`, `OpenedWindow`, the `WindowRequest` factory (`FnOnce + Send`) | off-thread `open_window`, worker threads, `OpenWake`, `PendingRequestState` and the keep-alive exit notifier (`flui-app`) | handle |
| `Doorbell`, `WindowWake` | IO-pool threads, zbus's own thread, the clipboard and drag threads | waker |

`OwnerSignal` holds no `Arc<dyn Platform>`: it hands requests to the loop, and the loop opens
windows and asks the exit policy again in its own turn, through the `Weak<dyn Platform>` the owner
thread keeps for it (an `OwnerSlot` until the per-host `Rc` step of §5 moves it into the loop).
`SharedPlatform` is removed. Of what it forwarded, another thread needed one request, asking the
exit policy again once a keep-alive service ends, which `PlatformProxy::request_exit_policy_reevaluation`
carries as a coalesced flag the owner's next turn settles (asked on the owner, it asks the
platform at once). Every other caller (`launch_value`,
`data_store`, `data_transfer`) is on the owner and reads `OwnerPlatform`; the file prompts and
shell dispatches it forwarded had no caller and stay on `Platform` for the system capability crate
(ADR-0151 §9).

### §3. The host runs its IO on the runtime its creator hands it

```rust
// flui-native
#[derive(Clone)]
pub struct NativeExecutor(Inner);                          // kind handle
enum Inner { Own(Arc<RuntimeOwner>), Borrowed(tokio::runtime::Handle) }
impl NativeExecutor {
    pub fn own() -> Self;                                  // a lazy 2-thread runtime, no threads until first use
    pub fn from_tokio(runtime: tokio::runtime::Handle) -> Self; // precondition: IO and time drivers enabled
    pub(crate) fn handle(&self) -> tokio::runtime::Handle; // the sources spawn on it
}
impl CurrentHost {
    pub fn start_with(self, executor: NativeExecutor) -> Result<Box<dyn Platform>, PlatformError>;
}
// flui-runtime (tier-internal, hosts only)
impl ExecutionServices {
    // None when the host injected its own pools (`Backend::Host`): FLUI runs no runtime then.
    pub fn io_runtime(&self) -> Result<Option<tokio::runtime::Handle>, SpawnError>;
}
```

`from_tokio` names `tokio::runtime::Handle` in the core host's public path, the one exception to
ADR-0151 §8's clause. The handle proves the runtime exists and lets the sources spawn on it without
a context of their own; it does not prove the drivers. That the runtime has its IO and time drivers
enabled is a documented precondition of `from_tokio`, met by the one caller that passes a borrowed
runtime: `flui-app` hands over the IO pool of `ExecutionServices`, built with `enable_all`. A
handle without them fails inside the source's task: tokio panics when the task first registers a
socket or a timer, the task ends with a `JoinError`, and the source reports itself unavailable as it
does when its service is missing; the owner thread does not panic. A run-time
`Handle::try_current()` check (option 2) would not prove more and would add a failure the host
reports at start. `BackgroundExecutor` becomes `pub(crate)` as `Inner::Own`, and its `block` and
`spawn` are deleted.

Only the Wayland and X11 hosts start sources on the runtime (XIM, IBus, logind, the settings
portal), so only they take an executor. The Windows file dialogs run on an STA thread of their own
and answer through a `tokio::sync::oneshot` that `Task<T>` waits on: they need no runtime, and no
runtime worker blocks on a dialog. The macOS, iOS, Android, web and headless hosts run no IO task.

`flui-app`'s runner starts the execution services before the host: `install_host_executors`
and `ensure_execution` move ahead of `platform()`, and on Wayland and X11 the runner calls
`start_with(NativeExecutor::from_tokio(runtime))` with the runtime `io_runtime` returns. Every other
host, and an application that brought its own executors through `with_executors` (`io_runtime`
returns `None`), starts with `start()`, which uses `own()`, so an embedding stays neutral and no IO
pool starts for a host that would not use it.

Without an application (`current_platform()`, `CurrentHost::start()`, `headless_platform()`,
`flui-native`'s tests, the dev dependencies of `flui-testing` and `flui-runtime`) the host uses
`own()`. The driver agent lives in `flui-app` and gets the application's runtime.

### §4. A source ends when its owner drops it

Each long-lived source (logind, the settings portal, the IBus bus) keeps the `JoinHandle` of its
task and aborts it in `Drop`, as it does today (`HostNoticeSource`, `PreferenceSource`, `Bus`).
Dropping the future closes what it holds: the logind delay inhibitor is a file descriptor, and its
close is what logind waits for; the bus socket's close ends the IBus session. No goodbye message is
owed, so there is no stop protocol and no budget of its own.

On the borrowed runtime the order at exit is the one ADR-0258 §3 already gives. The loop's sources
drop with the loop when the platform is released. If the pools of step 5 shut down first, the
runtime drops the source tasks itself: `ExecutionServices::shutdown` cancels its root token and
shuts the runtime down with `shutdown_timeout`, which drops every task at its `await` and waits
only for blocking threads, up to `exit.end()`, so a source task, finished or hung at an `await`, holds
neither the deadline nor `EXIT_WATCHDOG`. A spawn on a runtime that has shut down returns a
`JoinHandle` that resolves as cancelled; the source reports itself unavailable and does not panic.

### §5. Order of the change

Each step is its own PR, green on Linux CI, with the other hosts written from their
documentation and type-checked:

1. this record;
2. `NativeExecutor`, `CurrentHost::start_with`, `BackgroundExecutor` made `pub(crate)` without `block` and `spawn`, the Windows dialogs answering through a `oneshot`;
3. `ExecutionServices::io_runtime`, and `flui-app` starts the services first and hands the runtime over;
4. the thread bounds leave `Platform`, `Loop`, `Window::Shared`, `OpenWindows` and `OwnerHooks` in
   one change across every host, bounds only, with `cross-typecheck` green on every target before
   it merges;
5. one PR per host moves `Arc`/`Mutex` to `Rc`/`RefCell` inside the loop and windows and deletes
   its `unsafe impl Send/Sync`: headless, X11, Wayland, Windows, macOS, iOS, Android, web. Each
   host keeps an owner-local turn slot, where the callback of `on_wake` waits between turns, so
   that callback is not `Send` and other threads, which lock the owner signal to wake the owner,
   never reach it. With the last of them `run_shared` takes `Rc<Self>` and the callbacks of
   `on_keyboard_layout_change` lose `Send`;
6. `flui-platform`'s five traits lose `Send + Sync` (ADR-0323 §4) and `WindowFacets` holds `Rc`;
7. `OwnerSlot`, `OwnerCell` and `OwnerMap` are deleted.

### Consequences

- Good, because a window callback, a facet or a loop field may hold `Rc` and `Cell` state, and a
  registration from the wrong thread does not compile.
- Good, because a process runs one IO runtime, and its threads are named `flui-io`.
- Bad, because the core host's public API names a tokio type; it is one constructor, and
  `flui-native` is reached only from `flui-app` (ADR-0082).
- Bad, because step 4 touches every host at once: `Platform` has one implementation,
  `HostPlatform<H>`, for all of them.

### Confirmation

- `static_assertions::assert_not_impl_any!(HostPlatform<…>: Send, Sync)` and the trybuild cases
  `platform_is_not_send` and `window_is_not_send`, which replaces
  `an_input_callback_registered_off_the_owner_thread_is_refused`.
- `ui_pass` fixtures: a loop callback holding `Rc<Cell<_>>`, and an `impl PlatformHaptics`
  holding `Rc<Cell<u32>>` installed on a headless window.
- `the_host_runs_its_io_on_the_runtime_it_is_handed`: an X11 host started with a lent runtime
  spawns its sources there.
- Under Xvfb: an X11 application runs with `flui-io` threads and no `flui-native-bg` thread
  (`/proc/self/task/*/comm`).
- A source task parked at an `await` forever on a borrowed runtime is finished once its owner drops
  (`JoinHandle::is_finished`), and `ExecutionServices::shutdown` returns by its deadline with it
  still pending; on the fake bus of `lifecycle/linux/logind`, dropping the source closes the
  inhibitor descriptor.
- `send-positions.toml` drops the five traits' entries; `xtask send-impls` drops `MacOSLoop` and
  `WebLoop`.

## Pros and cons of the options

### 1. Owner-local host, tokio handle from the creator

- Good, because the thread is proven by types and the runtime is named by one; the drivers are a
  precondition met by its one borrowing caller.
- Bad, because a tokio type enters the core host's public path.

### 2. Owner-local host, `Arc<dyn Executor>` and a run-time context check

- Good, because the host's API stays runtime-neutral.
- Bad, because a host lane without tokio is a failure found at run time, and spawning through the
  lane does not give the sources a tokio context of their own.

### 3. Keep the thread-safe storage

- Good, because nothing in `flui-native` moves.
- Bad, because ADR-0323 §4's removals stay undone, five traits keep a bound no thread uses, and
  `OwnerSlot`'s run-time refusal and the `unsafe impl`s stay.

## More information

The design was weighed against the call sites listed in the context table; the first PRs of §5
carry the tests named under Confirmation. `docs/gaps.md` lists the record while it is not built.
