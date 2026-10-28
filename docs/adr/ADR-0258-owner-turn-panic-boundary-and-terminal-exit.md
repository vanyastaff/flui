# ADR-0258: A user-code panic is contained at the owner turn, and only a terminal failure ends the process, after the flush

- **Status:** Proposed. §4 (what an OS entry does with a panic), §6's retry bound and §7's exit
  budget are pending the owner's decision; their options are laid out and none is chosen. Not
  implemented: today a caught panic is reported and then resumed, and on Win32 it reaches the
  window procedure's boundary, which aborts.
- **Date:** 2026-10-28
- **Supersedes (on acceptance):** [ADR-0035](ADR-0035-lifecycle-consolidation-and-frames-enabled.md)
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
3. **the flush registry is written**: `FlushHost::flush_within(now + SERVICE_SHUTDOWN_DEADLINE)`,
   which waits for the write in flight and writes the latest unconfirmed bytes of every name
   (ADR-0259 §5). It runs whether or not the slot is occupied;
4. services are cancelled and joined against what is left of the same deadline;
5. the execution pools join with their grace;
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

### §4. What an OS entry does with a panic — pending owner decision

**Proposed — pending owner decision.** §1 and §2 keep every application panic away from an OS
entry: they are caught in `flui-app` before the call point. What reaches `os_entry` is a panic in
FLUI's own host code, or one a missing boundary let through. The question is whether such a panic,
when it is not a `BUG:` invariant, may still end the process with 101 after the flush, or ends it
where it is, as `os_entry` does today on Win32, AppKit and UIKit (`AfterPanic::Abort`). The options
are 4a, 4b and 4c under "Pros and cons of the options for §4" below.

The answer decides which rows of the exit-code matrix (`user_panic_exit_code_matrix`) expect 101
and which expect an abort, and what the native run of the window-procedure path asserts.

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
is the presentation's frame interval and `e = min(k − 1, 6)` for `k ≥ 1`, capped at one second. The
exponent is clamped before the shift, so the shift cannot overflow, and the `Duration`
multiplication is `checked_mul`, saturating to the cap. A healthy frame resets the streak.

The attempt is an entry in the presentation's deadline queue, which `UiRuntime::next_wake` already
reads, so the host's wake deadline carries it without new wiring, and a pump woken at the deadline
makes the attempt. Only a discrete user action, a key press or a pointer button, makes an attempt
early, once per window per interval; a signal write, a ticker, pointer motion and the failed paint's
own writes only confirm the deadline. In the steady state that is about one attempt a second, and
each failure is still one `frame_failure` report.

**Proposed — pending owner decision:** the bound a test holds this to, and how it is measured on
the virtual clock. The options are 6a, 6b and 6c under "Pros and cons of the options for §6"
below.

### §7. The exit budget — pending owner decision

**Proposed — pending owner decision.** The exit order of §3 is bounded by its constants today:
`SERVICE_SHUTDOWN_DEADLINE` (5 s, shared by the flush and the services) and
`EXECUTION_SHUTDOWN_GRACE` (5 s for each of the two pools, joined one after the other), 15 s in all.
Whether the contract names that bound, and where it lives, is open; the options are 7a, 7b and 7c
under "Pros and cons of the options for §7" below.

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
- Neutral, because `BUG:` invariants keep their meaning: a framework defect still ends the process
  where §4 says, and `CallbackPanic` carries `internal_invariant` so a handler can tell them apart.

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
  one row per path, its expectation for the window-procedure path set by §4.
- §5: `user_drop_panic_inside_an_unwind_is_retained` (a child: `dispose` panics and a future's
  `Drop` panics during the same unwind, without an abort).
- §6: `a_repeating_paint_panic_backs_off_but_keeps_retrying`,
  `a_single_paint_panic_redraws_without_input`, `a_pointer_move_does_not_bypass_the_backoff`,
  `a_failure_streak_past_thirty_three_keeps_the_one_second_cap`, and
  `the_runner_sleeps_until_the_failure_retry_deadline`.
- The native exit codes are confirmed by a dated run of the acceptance app through flui-driver's
  `os` lane: a panic in a Save handler leaves the process running, a panic in `dispose` on the last
  window close exits 101. How the panic is injected into a native process is pending the owner's
  decision; its options are under "Pros and cons of the options for injecting a panic into a
  native run" below.

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
- Bad, because the loop goes on pumping messages, if only to the default procedure, over host state
  the panic may have left torn, which is what the abort in `os_entry` prevents.
- Bad, because it adds a third `AfterPanic` form, a payload channel out of `Platform::run` and a
  `BUG:` test in the host, for panics that §1 and §2 already keep out of the entry when the
  application caused them.

#### 4b. OS entries keep aborting; 101 is the outcome of what `flui-app` catches

`os_entry` stays as it is. Every application panic is caught by §1 or §2 before the call point and
never reaches an entry, so application failures give the outcomes of §3. A panic that does reach an
entry is a host defect, or a missing boundary in `flui-app`, and ends the process there.

- Good, because the host keeps one rule at its boundary, and pumps nothing over torn state.
- Good, because no new host surface is needed.
- Bad, because a non-`BUG:` panic in FLUI's own host code aborts without the flush: the exit-code
  matrix has an abort row that a user can still reach through a framework defect.
- Bad, because a boundary missing in `flui-app` shows up as an abort rather than as 101; the matrix
  must have a row per OS-entry path to catch it.

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
real time. What it asserts is open.

#### 6a. A corridor of attempts over a window

After two seconds of warm-up with no input, run twelve seconds and count the reports in the last
ten: at least 8 and at most 12.

- Good, because the lower bound proves the loop wakes without input, and the upper one that it does
  not spin.
- Bad, because the numbers depend on `T` and the cap, and change when either does.

#### 6b. An upper bound only (N attempts a second)

- Good, because it states the property that matters, no hot loop, with one number.
- Bad, because a loop that stops retrying passes it; a separate test must prove the retry happens.

#### 6c. The exact schedule

Assert the instants of the first attempts (`T`, `2T`, `4T`, …) and that the interval holds at one
second past the exponent's clamp.

- Good, because it pins the formula itself, overflow clamp included.
- Bad, because any change of policy rewrites the test, even one that keeps both properties.

### Pros and cons of the options for §7

#### 7a. The bound is derived from the constants

The contract states the exit is bounded by `SERVICE_SHUTDOWN_DEADLINE` plus the pools' grace, 15 s
today, and the native run asserts that bound.

- Good, because there is one source of truth: the constants the code uses.
- Bad, because 15 s is long for a user who closed a window, and nothing makes it shorter.

#### 7b. A named exit budget the exit order shares

One constant (for example `SERVICE_SHUTDOWN_DEADLINE` plus one second) bounds the whole exit; each
step takes what the previous left.

- Good, because the bound is short and stated once, and a test can read the constant.
- Bad, because the pools then join with less than their grace when services are slow, and a
  compute job is abandoned sooner.

#### 7c. No bound in the contract

The tests assert only that the loop returns `Ok` and the process exits 0.

- Good, because nothing new is promised.
- Bad, because a regression that adds seconds to every exit passes.

### Pros and cons of the options for injecting a panic into a native run

#### Environment variable in the acceptance app's launcher

- Good, because one binary serves every row.
- Bad, because a fault switch read from the environment sits in the path of a binary that is also
  run normally, and reading it is process-global.

#### A separate fixture binary built with the fault seam

The acceptance app takes its faults as a constructor argument; a fixture binary builds it with the
fault set, and the `os` lane launches that binary.

- Good, because the shipped launcher has no fault switch, and the seam is the same one the headless
  rows use.
- Bad, because it is one more binary to build and keep in step with the launcher.

#### A driver action that injects the fault

flui-driver's agent, which every debug app hosts (ADR-0239), gains an action that arms a fault in
the next Save or `dispose`.

- Good, because no extra binary and no environment switch: only debug builds have the agent.
- Bad, because it adds a fault verb to the driver protocol (ADR-0238) for one test family.

## More information

The requirements behind this record: a callback panic keeps the process, a teardown panic exits
101 after the flush, and no abort is reachable from application code. Revisit §4 if a host gains an OS entry whose default answer is known to be
safe after any panic; revisit §6 when a presentation's frame interval is not fixed (a variable
refresh rate), since `T` is read from it.
