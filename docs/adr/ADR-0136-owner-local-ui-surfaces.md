# ADR-0136: UI surfaces are owner-local; one thread-boundary ledger

- **Status:** Accepted (§2; the other sections are not decisions until adopted)
- **Date:** 2026-10-06
- **Superseded-by:** [ADR-0182](ADR-0182-complete-owner-execution.md) for scheduler execution authority and frame-entry API.
- **Superseded-by:** [ADR-0175](ADR-0175-owner-local-scheduling-and-animation.md)
  for the transitional scheduler ownership and split post-frame queues;
  [ADR-0178](ADR-0178-notification-first-failure-custody.md) for listener failure custody.
- **Supersedes:** [ADR-0018](ADR-0018-async-builder-seam.md) D3 and D5 (where the driver's
  tasks live and what may be spawned), [ADR-0027](ADR-0027-owner-affine-ui-realms.md) §2
  (which UI capabilities are `!Send`), [ADR-0035](ADR-0035-lifecycle-consolidation-and-frames-enabled.md)
  §3's async pump API and [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md)'s
  lane-specific frame entry API. Their other decisions remain in force.
- **Related:** [ADR-0035](ADR-0035-lifecycle-consolidation-and-frames-enabled.md),
  [ADR-0047](ADR-0047-unified-execution-services.md),
  [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md),
  [ADR-0091](ADR-0091-one-owner-thread-isolated-realms-raster-thread.md)

This record states the decision section by section as the code adopting it lands. Section 2,
the UI runtime's ownership of its async tasks and owner-local frame state, is in the code; the
other sections (the thread-boundary ledger and its gate, the single post-frame queue,
owner-local controllers, listener panics, the ban on `unsafe impl Send`/`Sync` for UI types,
and the opt-in path to parallel layout) are added here when theirs does.

## Context

A UI runtime's UI runs on one owner thread (ADR-0027, ADR-0091 §1), yet its async driver was
`Send + Sync`: `BoxedTask` required `Send`, the task map sat behind a `Mutex` shared with every
waker, and `AsyncDriver` was an `Arc` clone of that state. Three consequences followed.

- A future spawned from a widget could not hold `Rc` state, `Signal<T>` or a `WriterSource`,
  so `FutureBuilder` required `Send` keys and payloads, and owner-local loaders (persistence)
  had to wrap their state in `Arc<Mutex<_>>` for no reason but the driver's bound.
- Every `AsyncDriver` clone was a strong owner. A widget state that leaked its handle kept the
  UI runtime's tasks, and every capture they held, alive after the UI runtime; the last drop could happen
  wherever that handle died.
- Frame entry points came in pairs (`drive_frame`/`drive_frame_with_lane`, `end_frame`/
  `end_frame_with_lane`, `execute_frame`/`execute_frame_with_lane`), the lane-less half
  draining only the shared post-frame queue, and the async poll lived in the scheduler, so a
  frame could be driven that polled or drained "nothing" of the UI runtime's owner-local work.

The initial implementation kept `UpdateScheduler` `Send`. ADR-0175 moves its UI
state to the owner thread and preserves only the independent wake capability
across threads.

## Decision

### 2. The UI runtime owns its owner-local frame state; handles are `Weak`; only wakes cross threads

- **`OwnerFrame`** (`flui-scheduler`, `#[doc(hidden)]`, `!Send`, not `Clone`) holds a UI runtime's
  owner-local post-frame queue and its task store. The UI runtime (`UiRuntime`, through
  `RuntimeServices`) and the headless binding (`HeadlessBinding`) hold the only strong
  reference. It replaces `LocalPostFrameLane`.
- **`AsyncDriver`** — what `LifecycleContext::async_driver` hands a widget — is `!Send` and a
  `Weak` handle to that store. `BoxedTask` has no `Send` bound; `TaskToken` is `!Send`.
  Futures are created, polled and dropped on the owner thread. A handle that outlives its
  UI runtime keeps nothing alive: spawning through it drops the future at once on the calling
  thread, logs a `warn!`, and returns an already-cancelled token.
- **Waking crosses threads, nothing else does.** A task's `Waker` is `Send + Sync` and holds a
  `Weak` to the store's cross-thread half only: a ready-id queue and the frame-request hook.
  A wake after the UI runtime is gone upgrades nothing. `UpdateScheduler::frame_waker()` returns a
  **`FrameWaker`** (`Clone + Send + Sync`, `request_frame()`), which holds a `Weak` to its own
  scheduler: it wakes its own UI runtime and no other, and is inert once the UI runtime is gone.
  `OwnerFrame::new` installs it as the task store's hook.
- **The frame latch is unchanged.** `request_frame` swaps `frame_scheduled` to `true` and
  fires the platform hook only on the `false → true` edge; the latch is cleared with a
  `SeqCst` swap *before* the poll (at begin frame, or by `finish_async_pump`), then the poll
  runs, then a live `end_of_frame` waiter re-issues its demand. A self-wake during the poll is
  never erased.
- **Every frame entry point takes the owner frame.** `handle_begin_frame(vsync, &OwnerFrame)`,
  `drive_frame(&OwnerFrame, ..)`, `execute_frame(&OwnerFrame)`, `end_frame(&OwnerFrame)` and
  `schedule_warm_up_frame(&OwnerFrame)`; the lane-less variants and the `*_with_lane` variants
  are gone, so no entry point polls or drains nothing. Tasks are polled in exactly two places:
  `MidFrameMicrotasks` inside `handle_begin_frame`, and `UiRuntime::pump_background`, which
  calls `finish_async_pump()` and then `OwnerFrame::poll_ready()`. An owner frame handed to
  another scheduler's drive is rejected before consuming pending frame demand.
- **Teardown retires explicitly, in its own order.** `OwnerFrame::retire` first closes both
  admission lanes, detaches their outgoing ownership, marks every task retired and removes
  the frame hook before invoking any destructor. It then drops the queued
  post-frame callbacks, then every task, on the owner thread, each under its own catch, keeps
  the first panic and retains later ones, and admits nothing afterwards. `UiRuntime`'s `Drop`
  calls it after closing its presentations and before it resumes any earlier failure, so the
  captures are dropped once, on the owner, even when a widget's `dispose` panicked first.
  During an existing unwind the values are retained without running their destructors, the
  same limit `TaskToken`'s `Drop` states: catching cannot contain an aggregate whose drop glue
  panics twice.
- `FutureBuilder<K, T, E>` requires `K: Clone + PartialEq + Debug + 'static` and
  `T, E: 'static`; `BoxedResultFuture` has no `Send` bound.

## Consequences

- Persistence and widget code spawn `!Send` futures directly; IO crosses threads as `Send`
  bytes through its own future (`IoFuture`) and completes the owner-local task by waking it.
- A late IO completion after a UI runtime is gone finds a dead `Weak` and does nothing; the task
  that would have received it was already dropped on the owner.
- The frame entry points' signatures are the ones the owner-local scheduler will keep: the
  queues `UpdateScheduler` still holds move into `OwnerFrame`, and the entry points do not
  change again.

## Alternatives rejected

- **A second, `!Send` lane beside the `Send` driver.** Two queues per family and an
  interleaving rule each; a widget would still have to choose.
- **Keeping `Arc` handles and documenting "do not leak".** Leaking a handle is ordinary (a
  closure captured in a long-lived object), and the cost — the UI runtime's tasks pinned to an
  arbitrary thread's lifetime — is exactly what the owner-thread model exists to prevent.
- **`send_wrapper`-style runtime checks.** They turn a compile error into a panic on the
  wrong thread.

## Validation

- `tests/thread_boundary_ui.rs` (`async_driver_stays_on_its_thread`): an `AsyncDriver` handed
  to `std::thread::spawn` does not compile.
- `crates/flui-testing/tests/async_driver.rs` (`owner_local_task_matrix`):
  `owner_local_future_completes_after_a_worker_wake`,
  `late_completion_after_ui_runtime_drop_drops_captures_on_the_owner`,
  `a_leaked_async_driver_holds_no_task_after_the_ui_runtime`.
- `crates/flui-widgets/tests/future_builder.rs`: `future_builder_accepts_an_owner_local_future`.
- `crates/flui-runtime/src/ui_runtime/tests/redraw_wake_routing.rs`:
  `frame_waker_wakes_the_ui_runtime_from_a_worker`.
- The latch tests keep their assertions: `finish_async_pump_reissues_a_stranded_live_waiters_demand`,
  `frame_scheduled_hook_fires_once_per_transition`,
  `lifecycle_reenable_edge_schedules_exactly_one_frame`,
  `headless_wake_from_another_thread_is_polled_on_the_frame_thread`, and the table in
  `crates/flui-scheduler/tests/wake_delivery.rs`.
