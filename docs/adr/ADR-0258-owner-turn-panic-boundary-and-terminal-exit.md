# ADR-0258: A user-code panic is contained at the owner turn, and only a terminal failure ends the process, after the flush

- **Status:** Accepted (2026-10-28). Not implemented: today a caught panic is reported and then
  resumed, and on Win32 it reaches the window procedure's boundary, which aborts; the exit order
  has no flush step, no shared deadline and no watchdog, and the frame retry has no backoff.
- **Date:** 2026-10-28
- **Supersedes:** [ADR-0035](ADR-0035-lifecycle-consolidation-and-frames-enabled.md)
  §5, its sentence "the first panic resumes after restoration and notification" only (a quit
  observer's panic is contained by §1 here and resumes only when §2 classifies it terminal); and the
  statement of `crates/flui-runtime/ARCHITECTURE.md` that a callback panic "still leaves the
  dispatch boundary".
- **Related:** [ADR-0127](ADR-0127-exceptional-path-retention.md) (what is retained on an
  exceptional path; the first failure stays first),
  [ADR-0123](ADR-0123-exceptional-presentation-close.md) (a close that holds a failure withdraws
  authority before it retains ownership),
  [ADR-0151](ADR-0151-platform-layer-boundary-and-names.md) §16 (`os_entry`, the boundary at every OS
  entry), [ADR-0254](ADR-0254-window-contract.md) decision 4 (one handler per window and per
  application; the call point resurfaces a handler's first panic),
  [ADR-0048](ADR-0048-frame-transaction-boundary.md) (a frame segment's panic is contained),
  [ADR-0259](ADR-0259-close-guard-close-reasons-and-flush-registry.md) (the flush registry the exit
  writes), [ADR-0260](ADR-0260-session-end-flushes-before-it-tears-down.md) (the session-end exit),
  [ADR-0097](ADR-0097-no-process-global-state-gate.md) (the terminal slot is host state, not a
  `static`)
- **Sources:** `rust-windowing/winit@0c7372808c59c0d46ae777959e65e773381dc9e9`
  `winit-win32/src/event_loop/runner.rs` (`catch_unwind`, `panic_error`) and
  `winit-win32/src/event_loop.rs` (`dispatch_peeked_messages`); the Rust Reference, *Panic*,
  "Unwinding across FFI boundaries" (<https://doc.rust-lang.org/reference/panic.html>); Qt 6,
  *Exception Safety* (<https://doc.qt.io/qt-6/exceptionsafety.html>); Flutter, *Handling errors in
  Flutter* (<https://docs.flutter.dev/testing/errors>); Kubernetes, *Pod Lifecycle*, "Termination of
  Pods" (<https://kubernetes.io/docs/concepts/workloads/pods/pod-lifecycle/>); systemd,
  *systemd.service*, `TimeoutStopSec=`
  (<https://www.freedesktop.org/software/systemd/man/latest/systemd.service.html>); GNU coreutils,
  *timeout invocation* (<https://www.gnu.org/software/coreutils/manual/html_node/timeout-invocation.html>);
  `chromium/chromium@b57678612bff5bf5c88c18146bdf5878292e6384`
  `content/browser/renderer_host/debug_urls.cc`,
  `chrome/android/javatests/src/org/chromium/chrome/test/crash/IntentionalCrashTest.java`,
  `chrome/browser/metrics/shutdown_watcher_helper.cc` and
  `chrome/browser/metrics/thread_watcher_report_hang.cc`

## Context and problem statement

A panic in application code has three fates in FLUI today, and the one a user meets depends on where
the code ran, not on what failed:

| Where | Today |
|---|---|
| `build` | the subtree becomes an `ErrorView`; the next frame builds |
| a frame segment (layout, paint) | contained per presentation (ADR-0048), reported as `SegmentPanic`, retried at the pace of `FallbackGate` with no backoff |
| an owner operation (input handler, post-frame callback, lifecycle observer, command) | `flui-runtime`'s owner delivery catches it, reports `FrameFailureKind::CallbackPanic` through `UiRuntime::report_contained_panic`, returns the runtime to its slot, and then resumes the first panic (`crates/flui-runtime/src/owner/delivery.rs`) |
| a window or application handler | the call point (`crates/flui-native/src/owner/call_point.rs`) resurfaces the panic into the OS entry that called it (`or_resume`) |
| an OS entry | `os_entry` logs the panic, and either answers the OS or ends the process (`AfterPanic`, `crates/flui-native/src/owner/panic_boundary.rs`); FLUI's Win32 window procedure, AppKit and UIKit entries end it (`crates/flui-native/src/owner/windows.rs`) |

So a tap handler that panics on Windows aborts the process with `0xC0000409`: the owner delivery
resumes, the window handler resumes, and the window procedure aborts. The closing path is the same:
the desktop `on_close` (`crates/flui-app/src/app/runner/desktop.rs`) catches a panic from closing its
presentation and resumes it from inside `WM_DESTROY`. At loop exit, `teardown_platform_ui_runtime`
(`crates/flui-app/src/app/runner/owner_dispatch.rs`) already contains each step and resumes the first
failure after the pools close, but nothing writes the persisted state the flush registry
(ADR-0259) holds, and a panic that aborted inside the loop never reaches it.

Which panics does the framework contain and keep the process running, which end it, and how does a
process that ends do so with its state written and a code a supervisor can read?

## Decision drivers

- a panic in an application's handler must not take the process down: one report, the UI runtime
  stays, the next action works;
- a failure while the last UI runtime is torn down cannot be continued from: the process ends, with
  the panic's text, and with what the application persisted written first;
- an `extern "system"`/`extern "C"` frame aborts on unwind; `os_entry` is the one boundary there
  (ADR-0151 §16) and nothing may bypass it;
- the first failure stays first, and later ones never replace it (ADR-0127 §2);
  `std::thread::panicking()` is false once a panic is caught, so no cleanup may rely on it;
- no `static`: the slot that carries a terminal failure is host state (ADR-0097);
- the frame path is synchronous: a retry is a deadline the loop wakes for, never a sleep.

## Considered options

1. Contain each owner operation in `flui-app`'s owner dispatch; a terminal class ends the process
   through a host slot after the exit flush.
2. Contain at the platform: the OS entry decides, for every panic, whether to answer and continue.
3. Contain in `flui-interaction`'s dispatcher, around gesture and keyboard handlers only.
4. Keep resuming: every caught panic leaves the operation, and the process ends wherever it lands.

## Decision

Chosen option: 1, because it is the only place that sees every kind of application callback and
still knows whether the UI runtime it belongs to can go on.

### §1. Each owner operation is contained on its own

Every owner operation (`RuntimeWork` taken from the host FIFO) runs under its own catch. A panic in
it that §2 does not classify terminal is reported once, as `FrameFailureKind::CallbackPanic {
message, internal_invariant }` with the `Contained` disposition, through
`UiRuntime::report_contained_panic(address, &payload)` at the operation's presentation address; the
UI runtime returns to its slot; the FIFO tail runs; and the panic does not resume. Its payload is
dropped under a catch, or retained (ADR-0127 §2) when its own destructor panics. A post-frame
callback's panic leaves the scheduler idle, so the next frame is scheduled normally.

Window and application handlers implemented by `flui-app` contain the operations they start in the
same way, so a contained panic never reaches the call point's `or_resume`, and never an OS entry.

The report reaches the application through `FrameFailureHandler`, which is documented as the
handler of a UI runtime's contained failures, not only of frames.

### §2. What is terminal

A failure is terminal when there is nothing left to continue: the last UI runtime is going, or the
loop has ended. Exactly these are terminal:

- the whole `ClosePresentation` operation of the last UI runtime: its close delivery (ADR-0259 §6)
  and its destruction;
- destroying a UI runtime at loop exit, the queued turns run there, and `on_quit`;
- a flush-registry failure or a service's panic at shutdown, when no terminal failure is held
  already.

A failure while a UI runtime that is not the last one closes is not terminal: it is one
`CallbackPanic` report, and what that close had not destroyed yet is retained (ADR-0123,
ADR-0127).

A terminal failure is not resumed where it is caught. Its payload goes to the host's terminal slot,
a field of the host state (`AppRuntime`), and the loop is asked to quit. The slot keeps the first
payload; a later one is logged and retained. An inert payload (`&'static str`, `String`) is kept as
it is; an opaque one is retained, and the slot keeps a `String` with its text
(`flui_foundation::panic::payload_text`).

The desktop `on_close` no longer resumes: a terminal failure from closing the last window is in the
slot, and `on_close` returns to the platform normally.

### §3. The exit order, and the exit code

After `Platform::run` returns, or panics (the run is called under a catch, and its panic is a
terminal failure):

1. the main window shuts down;
2. each remaining UI runtime is destroyed, and the queued turns run, each under its own catch;
3. the exit deadline is taken (§7), and **the flush registry is written**:
   `FlushHost::flush_within(exit.services())`, which waits for the write in flight and writes the
   latest unconfirmed bytes of every name (ADR-0259 §5). It runs whether or not the slot is
   occupied;
4. services are cancelled and joined against what is left of the same `exit.services()`;
5. the execution pools join against what is left of `exit.end()`;
6. the clipboard and the redraw window are released;
7. the code: a terminal payload in the slot resumes (`std::panic::resume_unwind`), so the process
   exits with 101 and the standard hook's text; an `AppRunError` returns `Err`; otherwise `Ok`.

`resume_unwind` does not call the panic hook again, so the text is printed once, where the panic
first happened. `Application::run` and `run_app` document this under `# Panics`.

The limits: iOS never returns from `UIApplicationMain`, so a terminal payload there is logged at
`applicationWillTerminate:` and not resumed; Android takes the same path as the desktop and is
checked by clippy only. A user `panic::set_hook` that re-enters a UI runtime runs before any FLUI
boundary and is not covered. A value whose own fields panic twice in their drop glue aborts before
any container regains control (ADR-0127).

### §4. An OS entry keeps its answer; 101 is the outcome of what `flui-app` catches

Chosen option for §4: 4b. `os_entry` and `AfterPanic` stay as they are (ADR-0151 §16): an entry
answers the OS after a panic, and the Win32 window procedure, the cloak hook and the AppKit and
UIKit entries that have no safe answer end the process. Application code reaches the platform only
through `flui-app`'s `AppHandler` and `WindowHandler` and the callbacks `flui-app` installs, and
every one of them runs the application's work under §1 and §2, so it returns to the call point
normally after an application panic: it never takes the call point's `or_resume` path, and no
application panic reaches an OS entry.

```rust
// flui-native, unchanged
pub(crate) enum AfterPanic<A, R> {
    Answer(fn(A) -> R),
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "ios"))]
    Abort,
}
// flui-app: the signatures are main's; each body is wrapped whole
impl flui_native::AppHandler for RunnerHandler { // and `WindowHandler for WindowRoutes`
    fn session_end(
        &self,
        _: &flui_native::OwnerPlatform,
        phase: flui_platform::SessionEndPhase,
    ) -> flui_platform::SessionEndAnswer {
        self.contained(flui_platform::SessionEndAnswer::Proceed, || answer_session_end(phase))
    }
    // … every method the same, each with the answer its entry gives after a failure
}
impl RunnerHandler {
    /// The whole body under one catch: a panic is classified by §2 (terminal → host slot and
    /// quit; otherwise one `CallbackPanic`), and the method returns `failed`. Never resumes.
    fn contained<R>(&self, failed: R, body: impl FnOnce() -> R) -> R;
}
```

The catch is total: it wraps each method's whole body, not only the application operations the
body dispatches (which §1 already contains one by one), so a defect in `flui-app`'s own routing
code is caught there too. What is left to reach an OS entry is `flui-native`'s own code.

What still reaches an OS entry is a defect of FLUI itself: a panic in the native host's own code
outside any handler call. It is a framework defect whatever its text, so the boundary does not read
`BUG:` there: every panic at an entry gets the entry's answer, or ends the process where the entry
has none.

Why the abort stays for a host defect, instead of a parked payload and 101 (4a):

- **What a flush would add is small.** The flush registry writes each publish as it arrives (one
  write in flight per name, ADR-0259 §5), and a write is a temporary file renamed over the target,
  so an abort leaves the previous or the new whole file. Parking a host panic to flush at exit
  saves at most the last publish still in flight, and only for framework defects.
- **The cleanup would run over the state that panicked.** §3's steps 1 and 2 destroy windows and UI
  runtimes, which re-enters the window procedure and the native host whose state the panic left
  torn. Qt supports recovering from an exception in its event machinery only to exit the loop and
  clean up, and calls GUI work after it unsafe (*Exception Safety*); a FLUI exit order is GUI work.
- **It would be a second exit mechanism.** A park form of `AfterPanic`, a parked state in the Win32
  and AppKit owners that answers `DefWindowProcW` and posts the quit, and a payload resumed out of
  `Platform::run`, all for a class §1 and §2 already empty of application panics. winit parks a
  panic in its Win32 runner and resumes it after the dispatch (`runner.rs`, `catch_unwind`;
  `event_loop.rs`, `dispatch_peeked_messages`) because winit's callbacks are the application's;
  in FLUI that role is `flui-app`'s, and §2's host slot is the park.
- **Fail fast keeps the evidence.** An abort leaves the OS crash report (Windows Error Reporting,
  the macOS crash reporter) with the process as the defect left it; the panic text is already in
  the log, written by `os_entry` before it ends the process. Chromium crashes the browser process
  on a browser defect and tests that path by crashing the real process
  (`debug_urls.cc`, `IntentionalCrashTest`); Flutter contains framework-level errors in callbacks
  and reports them (*Handling errors in Flutter*), as §1 does, and leaves engine-level defects to
  the engine.
- **An unwind cannot leave an `extern "system"` or `extern "C"` frame:** since Rust 1.81 it aborts
  (the Rust Reference, *Panic*). An answer or an abort is all an entry can do without the park.

The exit-code matrix therefore has no abort row reachable from application code. The total catch
is what makes that hold, and `an_application_panic_never_leaves_a_handler` proves it: one row per
method of `AppHandler` and `WindowHandler`, each with application code that panics inside it, each
failing if the panic leaves the method. The proof is the same on every host, because the methods
are `flui-app`'s and the call point is shared, so the native run does not repeat it per path; it
proves the outcomes a user sees (see Confirmation). A host defect aborting is a documented
outcome of a framework bug, not a row a user can reach.

### §5. A destructor does not resume a panic while user values live

`UiRuntime`'s `Drop` does not resume a panic while its fields still hold application values: the
post-frame lane, the `AsyncDriver` and the signal writers are held in `ManuallyDrop` or `Option` and
destroyed explicitly, each under a catch, before anything resumes. A `Drop` on a teardown path that
resumes (in `ui_runtime`, `owner_dispatch`, `element_tree`, `build_owner`, `scheduler`) either
follows this rule or hands its failure to its caller instead. A user `Drop` that panics during an
unwind already in progress is retained (ADR-0127 §2), never run.

### §6. A failing frame is retried with backoff

After a frame is dropped (`FailureDisposition::FrameDropped`), the presentation keeps a streak
`k: u32` (`saturating_add`) and schedules its next attempt at `T · 2^e` after the failure, where `T`
is the presentation's frame interval and `e = min(k − 1, 10)` for `k ≥ 1`, capped at
`FAILURE_RETRY_CAP` (one second). The exponent is clamped before the shift, so the shift cannot
overflow, and the `Duration` multiplication is `checked_mul`, saturating to the cap. The clamp is
10, not lower, so that the cap is what bounds the interval at every frame interval down to one
millisecond (`2^10 · 1 ms > 1 s`): with a clamp of 6, a 120 Hz presentation would settle at
533 ms and a 240 Hz one at 267 ms, and the steady rate would depend on the display. A healthy frame
resets the streak.

```rust
// flui-runtime
pub const FAILURE_RETRY_CAP: Duration = Duration::from_secs(1);
const FAILURE_RETRY_MAX_EXPONENT: u32 = 10;
fn failure_retry_delay(frame_interval: Duration, streak: NonZeroU32) -> Duration {
    let exponent = (streak.get() - 1).min(FAILURE_RETRY_MAX_EXPONENT);
    frame_interval
        .checked_mul(1 << exponent)
        .map_or(FAILURE_RETRY_CAP, |delay| delay.min(FAILURE_RETRY_CAP))
}
```

The attempt is an entry in the presentation's deadline queue, which `UiRuntime::next_wake` already
reads, so the host's wake deadline carries it without new wiring, and a pump woken at the deadline
makes the attempt. Only a discrete user action, a key press or a pointer button, makes an attempt
early, once per window per interval; a signal write, a ticker, pointer motion and the failed paint's
own writes only confirm the deadline. In the steady state that is one attempt per
`FAILURE_RETRY_CAP`, and each failure is still one `frame_failure` report.

**What a test holds it to (option 6d, the properties against the named cap).** The headless host's
virtual clock records the instant of each `frame_failure` report of a paint that always panics, with
no input; the rows run at frame intervals of 1/60, 1/120 and 1/240 s. The test asserts the
properties the requirement names, not the instants the formula yields:

- **backoff:** every interval is at least `T`, and below the cap each interval is at least the
  one before it;
- **bounded rate:** once an interval reaches `FAILURE_RETRY_CAP`, every later one lies in
  `[FAILURE_RETRY_CAP, FAILURE_RETRY_CAP + T)`, so the steady rate is at most one attempt per cap.
  Within that range the intervals may go up or down: an attempt lands on the first frame at or
  after its deadline, so the jitter is up to one frame interval and is not held to an order;
- **liveness:** over the ten caps that follow, at least nine reports arrive without input.

The rows read `FAILURE_RETRY_CAP` and `T` rather than a count such as "8 to 12 in ten seconds", so a
change of the cap or of the doubling keeps them, while a hot loop, a retry that stops, or a rate
that depends on the display fails them. The exact instants are not asserted: they restate the
formula, and a test that restates the code passes with the code's own mistake. The clamp is pinned
apart, by `a_failure_streak_past_thirty_three_keeps_the_one_second_cap`, which drives the streak
past the width of the shift.

### §7. One exit deadline bounds the framework's work at exit

The framework's part of the exit order, steps 3 to 6 of §3, runs against one deadline taken when
step 3 starts:

```rust
// flui-app, runner
pub(crate) const EXIT_BUDGET: Duration = Duration::from_secs(6);
pub(crate) const SERVICE_SHUTDOWN_DEADLINE: Duration = Duration::from_secs(5); // within EXIT_BUDGET
struct ExitDeadline { services: Instant, end: Instant }
impl ExitDeadline {
    fn start(now: Instant) -> Self; // services = now + SERVICE_SHUTDOWN_DEADLINE, end = now + EXIT_BUDGET
    fn services(&self) -> Instant;  // the flush, then the services
    fn end(&self) -> Instant;       // the pools and the native release
}
// flui-runtime: the pools join against an instant, not a grace each
impl ExecutionServices { pub fn shutdown(&self, deadline: Instant); }
```

The flush and the services keep the five seconds they have today, and the pools join each against
what is left of `end`, so they get at least one second and every second the services did not use.
`EXECUTION_SHUTDOWN_GRACE` is deleted: a grace per pool, joined one after the other, made the bound
the sum of the steps (15 s), and any new step would have added to it unseen.

Steps 1 and 2 run application code (destructors, `on_quit`), which cannot be interrupted, so the
deadline starts after them, as the session-end budget starts at the framework's own work
(ADR-0260).

**The whole exit is bounded by a watchdog.** Application code in steps 1 and 2 can block, and
nothing outside the process ends a desktop application whose user closed its last window: its
window is gone, and the process would linger, holding its files and its single-instance lock, until
the session ends. So the exit order has an end-to-end bound, kept apart from the budget:

```rust
// flui-app, runner
pub(crate) const EXIT_WATCHDOG: Duration = Duration::from_secs(15); // from step 1; > EXIT_BUDGET
pub const EXIT_CODE_HUNG: i32 = 124;
// flui-native: ends the process now, with no destructor, `atexit` handler or DLL detach
pub fn end_process(code: i32) -> !; // TerminateProcess(GetCurrentProcess()) on Windows, _exit elsewhere
```

When step 1 starts, `flui-app` starts a watchdog thread that waits on a channel for
`EXIT_WATCHDOG`; the exit order drops the sender when step 7 is reached, and the thread ends. If
the wait times out, the watchdog logs the step the exit order is in (contained, like any log), writes
the flush registry itself with `flush_within(now + 1 s)` (the registry is shared across threads,
holds no lock across application code and runs none, ADR-0259 §5), and calls
`end_process(EXIT_CODE_HUNG)`. `end_process` is not `std::process::exit`, because `exit` runs
`atexit` handlers and, on Windows, DLL detach under the loader lock, either of which the hung
thread may block. The code is 124, the one `timeout` from GNU coreutils returns when the command it
ran timed out, so a supervisor reads a hung exit apart from a panic (101) and a crash.

Fifteen seconds is the bound the teardown requirement set, and leaves application code nine
seconds beyond `EXIT_BUDGET`. The watchdog is the framework's only use of `end_process`; it runs on
the desktop and Android hosts, whose loops return, and not on iOS, whose `UIApplicationMain` never
does, nor on the web.

Systems that stop a process bound the whole stop the same way and end what is left: systemd sends
`SIGKILL` once `TimeoutStopSec=` passes (*systemd.service*), Kubernetes once the termination grace
period ends, Windows once `WM_ENDSESSION` returns (ADR-0260). Chromium's shutdown watchdog
(`shutdown_watcher_helper.cc`, `thread_watcher_report_hang.cc`) only reports a hang
(`DumpWithoutCrashing`) after a long per-channel timeout, and leaves ending the process to
whatever stops it; a FLUI application whose last window closed has no such outer stopper, so the
watchdog ends it.

A pool job cut at the deadline loses nothing the application can still read: the UI runtimes are
gone, the services have finished or were abandoned, and the pools cancel their root token first,
so a future stops at its next await point regardless of the grace. Only a compute closure that runs
past the deadline is abandoned mid-run, and the process ends under it as it does today past 5 s.

This is the shape a shared shutdown budget takes elsewhere: Kubernetes gives a pod one termination
grace period that its `preStop` hook and its `SIGTERM` handling share, and kills what is left when
it ends (*Pod Lifecycle*, "Termination of Pods").

### Consequences

- Good, because a panicking handler, post-frame callback, lifecycle observer or command costs one
  report, not the process, on every host.
- Good, because a process that must end writes what the application persisted, then ends with 101
  and the panic's text instead of `0xC0000409` and no flush.
- Good, because one classification (§2) decides continuing against ending, instead of the place the
  panic happened to be caught.
- Bad, because a contained panic can leave application state half-updated: rolling back a
  callback's external effects is the application's responsibility, as it is today.
- Bad, because terminal failures are held in a slot until the exit order runs: a hang in a service
  that ignores cancellation delays the exit code by the service deadline.
- Bad, because a defect in the native host's own code, at an entry with no answer, still aborts
  without the exit flush: what was published but not yet renamed into place is lost, and the code
  is the OS's abort code, not 101 (§4).
- Bad, because the pools get at least one second at exit instead of five each: a compute closure
  that runs longer is abandoned sooner (§7).
- Good, because no exit lasts longer than `EXIT_WATCHDOG`: a blocking application destructor
  costs the process code 124, after the registry is written, instead of a process that never
  ends (§7).
- Bad, because the watchdog ends the process without destructors: an application destructor that
  would have finished after 15 s does not run to its end.
- Neutral, because `BUG:` invariants keep their meaning: a framework defect inside an owner
  operation is contained like any panic, with `internal_invariant` set in its `CallbackPanic` so a
  handler can tell them apart, and one that reaches an OS entry ends the process there (§4).

### Confirmation

A child-process test (in the pattern of `opaque_frame_child`) runs each panic path; the parent
checks the child's exit, the order markers it printed and the absence of an abort. On the headless
host the old code also exits 101, by an unwind without teardown, so the marker that the registry
was flushed before the panic resumed is what tells the fix from the old code, not the code.

- §1: `a_panicking_tap_handler_reports_once_and_the_next_tap_works`,
  `a_panicking_post_frame_callback_reports_once_and_the_next_frame_builds`.
- §2, §3: `teardown_panic_flushes_the_registry_before_resuming`,
  `dispose_panic_on_last_window_close_resumes_after_flush`,
  `capture_drop_panic_keeps_the_first_failure_and_still_flushes`,
  `a_write_past_the_deadline_leaves_a_whole_file_and_exits_ok`, and `user_panic_exit_code_matrix`,
  one row per path of PANIC-POLICY's table; no row expects an abort.
- §4: `an_application_panic_never_leaves_a_handler` (`flui-app`, headless): one row per method of
  `flui-app`'s `AppHandler` and `WindowHandler`, each driven through the headless platform with an
  application operation that panics, and one more per method with the panic in `flui-app`'s own
  routing code before the operation is dispatched (the total catch); the row fails when the panic
  unwinds out of the platform's call (the call point's `or_resume`), which on Win32, AppKit and
  UIKit is the abort. `cargo xtask panic-boundary` keeps every OS entry behind `os_entry`
  (ADR-0151 §16).
- §5: `user_drop_panic_inside_an_unwind_is_retained` (a child: `dispose` panics and a future's
  `Drop` panics during the same unwind, without an abort).
- §6: `a_repeating_paint_panic_backs_off_but_keeps_retrying` (the three properties, one row per
  frame interval: 1/60, 1/120 and 1/240 s), `a_single_paint_panic_redraws_without_input`,
  `a_pointer_move_does_not_bypass_the_backoff`,
  `a_failure_streak_past_thirty_three_keeps_the_one_second_cap`, and
  `the_runner_sleeps_until_the_failure_retry_deadline`.
- §7: `a_hung_service_and_compute_job_end_within_the_exit_budget` (`flui-app`, headless runner): a
  service that ignores cancellation and a compute closure that never returns; the loop returns
  within `EXIT_BUDGET` plus one second of slack, measured on the wall clock (the pools join on real
  threads). The old code, 5 s plus 5 s per pool, fails it.
  `a_blocking_destructor_ends_the_process_with_the_hung_code` (a child whose application
  destructor never returns): the child exits with `EXIT_CODE_HUNG` within `EXIT_WATCHDOG` plus one
  second, and the registry bytes published before the close are on disk. The old code never exits,
  and the parent's timeout fails the row.
- The native exit codes are confirmed by a dated run of the acceptance app through flui-driver's
  `os` lane, which reads the exit code of the process it launched (`gone { kind: process, exit }`).
  The rows prove what a user of a native window sees, which the headless rows cannot: Alt+F4 and a
  programmatic close exit 0 within `EXIT_BUDGET`; a panic in the Save handler, reached by a real
  click, leaves the process running and the next real key works; a panic in `dispose` when the last
  window is closed by Alt+F4 exits 101, never `0xC0000409`; a panic in the note screen's `build`
  shows the error view and the window keeps answering input. The faults come from a fixture
  binary, the example `acceptance_app_faults` (added with the acceptance app),
  which builds the app with the fault named by its argument (`build-panic`, `save-panic`,
  `dispose-panic`) through the same constructor seam (`Faults`) the headless rows use; the
  launcher the other native runs use, the example `acceptance_app`, has no fault switch.

## Pros and cons of the options

### 1. Per-operation containment in `flui-app`'s owner dispatch, terminal slot in the host

- Good, because every kind of callback passes through one dispatch: input, post-frame, lifecycle,
  commands, close.
- Good, because the dispatch knows whether the UI runtime is the last, which is what decides
  continuing against ending.
- Bad, because a post-frame callback's panic must leave the scheduler idle, which the dispatch has
  to ensure explicitly.

### 2. Contain at the platform's OS entry

- Good, because it is one place per host.
- Bad, because the platform does not know which UI runtime failed or whether it was the last; it
  can only answer the OS and continue, or end.
- Bad, because continuing after a panic mid-`WM_DESTROY` or mid-dispatch pumps more messages over
  state the panic may have torn, the reason `os_entry` aborts there.

### 3. Contain in `flui-interaction`'s dispatcher

- Good, because the gesture arena and key dispatch are where most handlers run.
- Bad, because post-frame callbacks, lifecycle observers, commands and close are not covered.

### 4. Keep resuming

- Good, because nothing changes.
- Bad, because a handler panic aborts the process on Win32, AppKit and UIKit, with nothing flushed.

### Pros and cons of the options for §4

#### 4a. An OS entry parks a non-`BUG:` panic and the process exits 101 after the flush

`os_entry` gains an answer for the window procedure that stores the payload in the owner's state,
answers `DefWindowProcW` from then on and posts the quit; `Platform::run` hands the payload back when
the loop ends, and §3's exit order resumes it. A `BUG:` payload still aborts.

- Good, because every non-`BUG:` panic, wherever caught, ends with the flush and 101.
- Bad, because the exit order then destroys windows and UI runtimes over the host state the panic
  left torn, re-entering the window procedure it parked.
- Bad, because it adds a park form of `AfterPanic`, a parked state in the Win32 and AppKit owners
  and a payload resumed out of `Platform::run`: a second exit mechanism beside §2's host slot, for
  panics that §1 and §2 already keep out of the entry when the application caused them.
- Bad, because telling a `BUG:` payload from another is arbitrary at the entry: a `RefCell` borrow
  error or an index out of range in host code is as much a framework defect as a `BUG:` expect,
  and would get the flush the other does not.
- Bad, because what the flush adds is the last publish still in flight: the registry writes each
  publish as it comes, and a rename leaves a whole file either way.

#### 4b. OS entries keep their answer or abort; 101 is the outcome of what `flui-app` catches (chosen)

`os_entry` stays as it is. Every application panic is caught by §1 or §2 before the call point and
never reaches an entry, so application failures give the outcomes of §3. A panic that does reach an
entry is a host defect, or a missing boundary in `flui-app`, and ends the process there.

- Good, because the host keeps one rule at its boundary, and runs nothing over torn state.
- Good, because no new host surface is needed: the boundary and its gate stand as they are.
- Good, because the abort leaves the OS crash report of a framework defect, with the panic text
  already logged by `os_entry`.
- Bad, because a panic in FLUI's own host code, at an entry with no answer, aborts without the exit
  flush.
- Bad, because a boundary missing in `flui-app` shows up as an abort rather than as 101; a row per
  handler method (`an_application_panic_never_leaves_a_handler`) catches it on the headless host,
  where the same escape unwinds out of the platform's call.

#### 4c. A terminal failure flushes and then ends the process where it is caught

Wherever a terminal failure is caught, inside an entry or not, the flush registry is written
inline and the process calls `std::process::exit(101)` there.

- Good, because the code is 101 everywhere and the flush happens even inside an entry.
- Bad, because `exit` skips the destructors of `main` and of every frame above it, and the
  services and pools are not joined.
- Bad, because the flush then runs inside a window procedure, the place `os_entry` avoids running
  more code.

### Pros and cons of the options for §6

The schedule is deterministic on the headless host's virtual clock, so a test can drive it without
real time.

#### 6a. A corridor of attempts over a window

After two seconds of warm-up with no input, run twelve seconds and count the reports in the last
ten: at least 8 and at most 12.

- Good, because the lower bound proves the loop wakes without input, and the upper one that it does
  not spin.
- Bad, because the numbers depend on `T` and the cap, and change when either does.
- Bad, because a count at one frame interval passes a rate that depends on the display, which the
  earlier clamp of 6 produced at 120 and 240 Hz.

#### 6b. An upper bound only (N attempts a second)

- Good, because it states the property that matters, no hot loop, with one number.
- Bad, because a loop that stops retrying passes it; a separate test must prove the retry happens.

#### 6c. The exact schedule

Assert the instants of the first attempts (`T`, `2T`, `4T`, …) and that the interval holds at one
second past the exponent's clamp.

- Good, because it pins the formula itself, overflow clamp included.
- Bad, because the test restates the code: a mistake in the formula is copied into the expectation.
- Bad, because any change of policy rewrites the test, even one that keeps both properties.

#### 6d. The properties against the named cap, at several frame intervals (chosen)

Backoff (every interval at least `T`, non-decreasing below the cap), bounded rate (once at the
cap, every interval in `[cap, cap + T)`, in any order) and liveness (at least nine reports in ten caps without input), read against
`FAILURE_RETRY_CAP` and `T`, at 60, 120 and 240 Hz.

- Good, because one table holds both halves of the requirement, no hot loop and no stall.
- Good, because a change of the cap or of the doubling keeps the test, and a rate that depends on
  the display fails it.
- Bad, because the cap becomes a public constant of `flui-runtime`, read by the tests.

### Pros and cons of the options for §7

#### 7a. The bound is derived from the constants

The contract states the exit is bounded by `SERVICE_SHUTDOWN_DEADLINE` plus the pools' grace, 15 s
today, and the native run asserts that bound.

- Good, because the constants the code uses are the only source.
- Bad, because the bound is a sum: each new step with a grace of its own adds to it unseen.
- Bad, because 15 s is long for a process whose last window has closed: a relaunch of a
  single-instance application, an installer or an update waits on it.

#### 7b. One exit deadline the exit order shares (chosen)

`EXIT_BUDGET`, six seconds, bounds steps 3 to 6; the flush and the services keep their five, and
the pools take what is left. `EXIT_WATCHDOG`, fifteen seconds from step 1, bounds the whole exit,
application code included, and ends the process with `EXIT_CODE_HUNG` after writing the registry.

- Good, because the bound is stated once, holds by construction, and a test can read it.
- Good, because the flush keeps the time it has today.
- Good, because the end-to-end bound the requirement set survives a blocking application
  destructor.
- Bad, because the pools join with less than their former grace when services are slow, and a
  compute closure that runs past the deadline is abandoned sooner.

The end-to-end bound had two other forms. Without a watchdog, a blocking destructor keeps an
invisible process alive until the session ends. A watchdog that only reports, as Chromium's does,
leaves the same process alive; it suits a program something else will stop.

#### 7c. No bound in the contract

The tests assert only that the loop returns `Ok` and the process exits 0.

- Good, because nothing new is promised.
- Bad, because a regression that adds seconds to every exit passes.

### Pros and cons of the options for injecting a panic into a native run

#### Environment variable in the acceptance app's launcher

- Good, because one binary serves every row.
- Bad, because a fault switch read from the environment sits in the path of a binary that is also
  run normally, and an environment is inherited: a variable left in a shell reaches every later
  run of the launcher, the persistence and text-input runs with it.

#### A separate fixture binary built with the fault seam (chosen)

The acceptance app takes its faults as a constructor argument; a fixture binary,
the example `acceptance_app_faults`, reads the fault from its own argument in `main` and builds the
app with it, and the `os` lane launches that binary with the argument.

- Good, because the launcher of the other native runs has no fault switch, and the seam is the
  same one the headless rows use.
- Good, because an argument belongs to one launch; nothing is inherited.
- Good, because the panic is triggered by OS input on the real control, so it travels the window
  procedure as a user's would, as Chromium's crash tests drive the real browser process to its
  crash (`IntentionalCrashTest`).
- Bad, because it is one more example to build; it shares the app's module and adds only a `main`.

#### A driver action that injects the fault

flui-driver's agent, which every debug app hosts (ADR-0239), gains an action that arms a fault in
the next Save or `dispose`.

- Good, because no extra binary and no environment switch: only debug builds have the agent.
- Bad, because it adds a fault verb to the driver protocol (ADR-0238) for one test family, and the
  agent would have to know the application's fault seam.

## More information

The requirements behind this record: a callback panic keeps the process, a teardown panic exits
101 after the flush, and no abort is reachable from application code.

Revisit §4 if a host gains an OS entry whose default answer is known to be safe after any panic,
or if the flush registry stops writing each publish as it comes (a host defect would then lose more
than the write in flight). Revisit §6 when a presentation's frame interval is not fixed (a variable
refresh rate), since `T` is read from it. Revisit §7's numbers after a dated native run measures
the exit on a slow disk.
