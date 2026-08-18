# ADR-0049: Task, worker, and service lifecycles

*Background work is classified by lifetime, and every unit has a named owner and an explicit end: one-shot **tasks** and recurring **workers** are owned by `#[must_use]`, cancel-on-drop handles with deadline-bounded join evidence; application-lifetime **services** are owned by the runtime's registry, declare whether the last window closing stops the app, and are shut down by a staged cancel → bounded-join → evidence pass that runs before the execution pools close. There is no fire-and-forget spawn and no `detach()`.*

---

- **Status:** Accepted (2026-08-18)
- **Date:** 2026-08-18
- **Deciders:** @vanyastaff
- **Scope:** background-work lifecycles — `crates/flui-app/src/app/lifecycle.rs`, `AppRuntime`'s service registry and exit consult, `AppConfig::with_service`, the loop-exit teardown staging in `app/runner.rs`
- **Related:** [ADR-0047](ADR-0047-unified-execution-services.md) (the execution lanes these lifecycles run on); [ADR-0027](ADR-0027-owner-affine-ui-realms.md) (concurrency topology is a sanctioned leapfrog zone — Flutter is not the reference); [Runtime Architecture Execution Plan](../research/2026-08-01-runtime-architecture-execution-plan.md) ("Define durable service lifecycle and graceful application shutdown"); `docs/runtime-contract.toml` (`task-worker-service-lifecycles`)
- **Issue:** [#558](https://github.com/vanyastaff/flui/issues/558) — on the Runtime.1 critical path after unified execution services (#557)

---

## Context

ADR-0047 gave `AppRuntime` bounded compute and IO lanes with admission, cancellation, and a
deadline-bounded pool shutdown — but no *ownership* model above them. `spawn_compute`/`spawn_io`
are fire-and-forget: no handle, no join evidence, no per-unit cancellation, no notion of work
that must outlive its spawn site or hold the application open. The execution-plan taxonomy calls
for four distinct concepts (scoped Task, reusable Worker, durable Service, optional
ProcessWorker) precisely because those lifetimes need different delivery and shutdown
guarantees; the issue adds the application-policy half — editor-like last-window exit and
messenger-like background lifetime under one contract, deterministic shutdown deadlines, and
late results that cannot revive a closed realm.

The market's structured-concurrency consensus (Kotlin coroutines, Swift structured tasks,
`smol`'s cancel-on-drop `Task`) is that *unowned* concurrency is the defect: work should be
owned by a scope or handle whose end is the work's end. Tokio's detached-by-default
`JoinHandle` is the counterexample this design deliberately rejects.

## Decision

### Four lifetimes, each with a named owner

| Class | Lifetime | Owner | Ends by |
|---|---|---|---|
| **Task** (`TaskHandle<T>`) | one-shot | whoever holds the handle | completion, explicit `cancel()`, or drop |
| **Worker** (`WorkerHandle<I, O>`) | recurring, submission-driven | whoever holds the handle | drop (cancel-on-drop) |
| **Service** (`ServiceDefinition`) | application-lifetime | `AppRuntime`'s `ServiceRegistry` | staged registry shutdown at loop exit |
| **ProcessWorker** | process-external | — | **not implemented** (see Consequences) |

**Cancel-on-drop, no detach — explicitly.** Dropping a task or worker handle requests
cancellation: cooperatively for compute (delivered before start and at every
`TaskContext::is_cancelled` check), at the next await point for IO (the future is dropped,
destructors run). There is deliberately no `detach()` and no anonymous spawn: work that must
outlive every natural owner is a *service* with a name and a registry entry. The handles are
`#[must_use]`; a handle parked in a struct field is a deliberate owner (the lint cannot see
fields — the recorded "linear token in a struct field" trap — so the doc states it rather than
pretending the lint covers it).

### Join evidence on dedicated paths, never shared queues

Every task and every service reports its outcome (`Completed(T)` / `Cancelled` / `Panicked`)
through its own capacity-one channel created at spawn — the plan's "dedicated reliable
completion paths rather than ordinary bounded inboxes". Panics are contained at the lifecycle
boundary (`catch_unwind` around compute bodies, a poll-level catch for futures) and become
evidence instead of executor unwinding. Every join is deadline-bounded (`join_within`; a
timeout returns the still-owning handle) — there is no unbounded blocking join anywhere on the
surface.

### Workers: generation-stamped, latest-wins, pull-only

A worker owns a capacity-one input slot and a capacity-one result slot. `submit` never blocks
and never queues: an unprocessed input is *replaced* (invalidations coalesce) and every
submission gets a strictly increasing `WorkerGeneration`; an uncollected result is replaced by
a newer one, so stale results drop structurally rather than arriving late. Results are
pull-only (`try_latest`), so cross-thread completion commits only at an anchor the owner
chooses. The pump job occupies pool capacity only while processing; an idle worker costs
nothing.

### Services: declared lifetime, registry ownership, typed events

`AppConfig::with_service(ServiceDefinition::new(name, lifetime, factory))` declares a service;
the desktop bootstrap starts each one once the realm install has resolved the loop's execution
services, and a start failure fails the bootstrap (a declared service is load-bearing, not
optional). `ServiceLifetime` is the editor/messenger split: `StopsWithLastWindow` services
never hold the loop; a running `KeepsAppAlive` service vetoes `AppRuntime::should_exit` after
the last window closes, and the veto lifts when the service completes — after which the loop
exits on the next exit-policy consult, or the embedder calls `Platform::quit` explicitly.

Services publish through `service_events`: a typed, bounded, latest-relevant ring —
`publish` never blocks (oldest events drop under pressure) and the receiver is pull-only, so a
service can never wake, mutate, or re-enter UI state. When the receiving side dies, every
later publish is `PublishError::OwnerGone`: a late service result after teardown is
structurally inert — nothing exists for it to revive.

Tasks and workers reach application code in this slice through `ServiceContext::spawner()`
only — a `TaskSpawner` holding the execution services *weakly*, so a spawner outliving its
loop refuses with `ShuttingDown` instead of keeping dead pools alive. Never ambient: no
global, no thread-local.

### Staged shutdown, ordered before the pools

Loop-exit teardown (`teardown_platform_realm`) now runs two stages in a load-bearing order:

1. **Service shutdown** (`ServiceRegistry::shutdown`): stop admission → cancel *every*
   service first (flush windows overlap, they do not serialize) → join each against **one
   shared deadline** → per-service evidence (`Completed` / `Panicked` / `DeadlineExceeded` /
   `Abandoned`), logged with names.
2. **Pool shutdown** (ADR-0047, unchanged): stop admission → cancel the root token → join
   bounded per pool.

The order is the whole point: the pool stage hard-drops any still-running future at its next
await point, so the cooperative pass must come first or no service ever gets its flush window
— pinned by a teardown test that fails when the two stages are swapped. A service that
ignores cancellation costs at most the shared deadline and is reported, never waited on
unboundedly; shutdown never blocks on any optional-work queue (completion channels are
dedicated and capacity-one; event rings drop rather than block). Every lifecycle signal is a
child of the pools' root cancellation token, so even the last-resort `Drop` teardown path
reaches all outstanding work.

The registry is loop-scoped like the pools: hot-restart never touches running services; a
second loop on the same thread reopens admission at its realm install, mirroring the
execution slot's reset.

## Alternatives considered

- **Detach-on-drop handles (tokio's `JoinHandle` shape).** Rejected: detached work has no
  owner, which is exactly the "shipped seams never wired"/orphaned-work failure class this
  repo keeps paying for. Kotlin/Swift/smol all landed on ownership; so does this.
- **Cancel-on-drop with an explicit `detach()` escape hatch (smol).** Rejected for the same
  reason but at one remove: every `detach()` call site is a fire-and-forget with extra steps.
  The escape hatch here is *naming the work as a service*, which keeps an owner.
- **Bounded queues for worker inputs/results instead of latest-wins slots.** A queue depth
  above one only defers the same drop decision while adding latency and a blocking/full edge
  precisely at shutdown ("shutdown never blocks on a full optional-work queue"). UI-flavored
  recurring work wants the newest input and the newest result; callers that need every input
  processed submit the next one on collecting the previous result.
- **Racing service futures against their token in the registry wrapper.** Rejected: it would
  hard-drop the service at cancel time, deleting the flush window that is the entire reason
  services get a *staged* shutdown. The hard stop already exists one stage later.
- **A `JoinEvidence` callback/observer API instead of per-unit channels.** More machinery,
  and it recreates a shared delivery path with head-of-line concerns; capacity-one channels
  per unit are cheap and independently reliable.
- **Widget-tier spawning capability now.** Deferred, not rejected: a `BuildContext`-acquired
  handle must follow the ADR-0018/0021 acquisition discipline and add its token to the
  frame-capability-scope checker; it deserves its own slice rather than riding this one.

## Consequences

- The `expect(dead_code)` ratchet on `app/execution.rs` is retired on native targets — the
  lifecycle layer is the lanes' production consumer (bootstrap → services → spawner → lanes,
  teardown → staged shutdown). It survives narrowed to wasm32, where the lifecycle layer does
  not exist yet.
- `ExecutionServices` is now held in an `Arc` by `AppRuntime` so lifecycle handles can hold
  it weakly; shutdown semantics are unchanged.
- **Deliberately not in this slice, tracked under #558:** ProcessWorker (FLUI has no process
  plumbing to build on); close-request veto/defer for unsaved work (needs a
  `CloseRequested` interception seam in the platform close path); journaled recoverable
  state; the widget-tier capability above; wasm32 lifecycles.
- A breaking reshape of these surfaces when the deferred slices land is expected and
  preferred over shims (`experimental` classification in `docs/runtime-contract.toml`).
