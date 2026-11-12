# ADR-0136: UI surfaces are owner-local; the compiler proves what crosses a thread

- **Status:** Accepted (2026-10-28)
- **Date:** 2026-10-06
- **Amended:** 2026-10-11 — later accepted owner APIs are distinguished from this record's historical signatures and unadopted proposals.
- **Revised:** 2026-11-03 — the gates the owner kept when rejecting the thread-boundary ledger
  are a numbered decision (§3) instead of a clause under "Alternatives rejected", and the title
  says so.
- **Superseded-by:** [ADR-0182](ADR-0182-complete-owner-execution.md) for scheduler execution authority and frame-entry API.
- **Superseded-by:** [ADR-0175](ADR-0175-owner-local-scheduling-and-animation.md)
  for the transitional scheduler ownership and split post-frame queues;
  [ADR-0178](ADR-0178-notification-first-failure-custody.md) for listener failure custody.
- **Superseded-by:** [ADR-0281](ADR-0281-a-destructor-that-panics-during-an-unwind-aborts-as-in-std.md),
  §2's teardown item only where it retains later panics and, during an unwind, the values without
  running their destructors: they are destroyed under the teardown's containment.
- **Superseded-by:** [ADR-0323](ADR-0323-a-value-stays-on-the-owner-thread-unless-its-signature-names-what-crosses.md) for §3 (the two
  gates and the kinds of value that cross a thread).
- **Supersedes:** [ADR-0018](ADR-0018-async-builder-seam.md) D3 and D5 (where the driver's
  tasks live and what may be spawned), [ADR-0027](ADR-0027-owner-affine-ui-realms.md) §2
  (which UI capabilities are `!Send`), [ADR-0035](ADR-0035-lifecycle-consolidation-and-frames-enabled.md)
  §3's async pump API and [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md)'s
  lane-specific frame entry API. Their other decisions remain in force.
- **Related:** [ADR-0035](ADR-0035-lifecycle-consolidation-and-frames-enabled.md),
  [ADR-0047](ADR-0047-unified-execution-services.md),
  [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md),
  [ADR-0091](ADR-0091-one-owner-thread-isolated-realms-raster-thread.md)

§2 and §3 are this record's decisions. §2's UI-runtime ownership of async tasks and weak
owner-local handles remains; the scheduler execution API and transitional
queue/notification descriptions are superseded by ADR-0182, ADR-0175 and ADR-0178
in the scopes above. The thread-boundary ledger and opt-in parallel layout were
rejected by the owner on 2026-10-28 (see Alternatives rejected); §3 is what that
decision kept.

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

The following entry-point and queue signatures describe the accepted migration's
original shape. Current complete-turn execution is ADR-0182; current scheduler
ownership and the unified owner-local post-frame queue are ADR-0175. The remaining
weak-handle and thread-boundary rules in this section still apply.

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
  `OwnerFrame::try_new` installs it as the task store's hook.
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

### 3. A value crosses a thread because the compiler proves it may; two gates hold the exceptions

**Superseded-by:** [ADR-0323](ADR-0323-a-value-stays-on-the-owner-thread-unless-its-signature-names-what-crosses.md) §2 and §3, which
state the gates and the kinds of value that cross.

§2 holds because the compiler refuses an owner-local value on another thread. Nothing is made
`Send` or `Sync` by assertion, and the places that do cross are lists that only shrink:

- **`cargo xtask send-impls`** lists every `unsafe impl Send` and `unsafe impl Sync` in the
  workspace in `tools/xtask/allowlists/send-impls.toml`; an unlisted impl fails the gate, and the
  list only shrinks. The impls that stay wrap an OS handle the platform guarantees for another
  thread, each with its `SAFETY:` comment. `deny.toml` bans `send_wrapper` and `fragile`, which
  would turn the compiler's refusal into a panic on the wrong thread.
- **`cargo xtask send-positions`** lists every `Send` and `Sync` position an author wrote in the
  public API of `flui` and `flui-sdk` (a supertrait, a bound, a `dyn … + Send`) in
  `tools/xtask/allowlists/send-positions.toml`, read from the nightly rustdoc JSON that
  [ADR-0089](ADR-0089-upstream-types-in-stable-signatures.md) §6's loader produces, in CI's
  `api-contracts` job. An unlisted position fails the gate, and so does an entry the code no
  longer has. A position is admitted only where a value really crosses a thread: a caught
  panic's payload, a waker or what it carries, a worker's result, or a handle meant for another
  thread. The change that writes it lists it, naming that kind as the entry's
  `crosses = "panic-payload" | "waker" | "worker-result" | "handle"`, which the gate parses and
  refuses any other value of. Every other entry only goes.

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
- **A thread-boundary ledger** (rejected by the owner, 2026-10-28): every `Send` type and
  bound listed with a flip-or-keep reason. The reasons would be prose a gate cannot check.
  What remains is the gate without the ledger (§3).
- **Opt-in parallel layout through an `IndependentLayout` marker** (rejected by the owner,
  2026-10-28). It would put a `Send` position back into the render-object surface for a
  speed-up nothing measures; [ADR-0091](ADR-0091-one-owner-thread-isolated-realms-raster-thread.md)
  §2 keeps layout inside one UI runtime serial.

## Validation

- §3: `cargo xtask send-impls` runs in `cargo xtask checks` and `cargo xtask send-positions` in
  CI's `api-contracts` job, each with its `--self-test`; `cargo xtask deps` (cargo-deny over
  `deny.toml`) holds the crate bans; xtask's `send_positions_list_entry_names_one_of_the_kinds_that_cross`
  refuses a `crosses` kind this section does not name.
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
