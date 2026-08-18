# ADR-0047: Unified execution services under `AppRuntime`

*Background execution is one loop-scoped service owned by `AppRuntime`, not a per-platform possession: work is classified by deadline and behavior (frame-required compute, asynchronous compute, IO), admission is bounded, shutdown cancels-then-joins under a deadline, and an embedded host can inject its own pools — in which case FLUI never constructs its default ones. The per-platform full-core `BackgroundExecutor` is defanged (lazy, small) ahead of its removal.*

---

- **Status:** Accepted (2026-08-18)
- **Date:** 2026-08-18
- **Deciders:** @vanyastaff
- **Scope:** background execution topology — `crates/flui-app/src/app/execution.rs`, `AppRuntime`'s ownership/teardown wiring, `AppConfig::with_executors`, `crates/flui-platform/src/executor.rs`
- **Related:** [ADR-0027](ADR-0027-owner-affine-ui-realms.md) (runtime/scheduling topology is a sanctioned leapfrog zone — Flutter is not the reference here); [Runtime Architecture Execution Plan](../research/2026-08-01-runtime-architecture-execution-plan.md) ("Unify worker, I/O, and service execution with host injection"); [Runtime Dependency Adoption Guide](../research/2026-08-01-runtime-dependency-adoption-guide.md) (`tokio-util` adoption, "another async runtime: do not add"); `docs/runtime-contract.toml` (`execution-services-owned-by-app-runtime`)
- **Issue:** [#557](https://github.com/vanyastaff/flui/issues/557) — on the Runtime.1 critical path between singleton retirement (#553) and the task/worker/service lifecycles (#558) / threaded raster lane (#559)

---

## Context

Before this change, background execution was a per-platform possession with no policy:

1. **Every platform backend constructed a full-core Tokio runtime at platform init** (`BackgroundExecutor::new()` with `num_cpus::get()` workers, eagerly, in `WinitPlatform`/`WindowsPlatform`/`MacOSPlatform` constructors) — and production never called `Platform::background_executor()` at all. Managed runs paid a core-count thread pool for nothing, and any second pool (a host's, flui-assets' bridge) stacked on top.
2. **Priority was informational.** `spawn_with_priority` discarded its argument; the public `Priority` enum promised behavior that did not exist.
3. **No injection seam.** An embedded host running its own executor (a game engine, an editor) had no way to lend it to FLUI; embedding FLUI meant oversubscribing the machine.
4. **No admission, cancellation, tracking, or shutdown contract.** `Task::drop` did not cancel; nothing joined outstanding work at exit; queues were unbounded.

The runtime architecture study's target is explicit: *"Work is classified by deadline and behavior, not arbitrary user priority: frame-critical compute, asynchronous compute, I/O, and durable services"*, with host-provided executor/task pools as a required embedding capability. ADR-0027 sanctions diverging from Flutter here — Flutter has no contract for this at all.

## Decision

### One owner, no ambient reach

`AppRuntime` — the loop-scoped composition root — owns exactly one `ExecutionServices` value (`app/execution.rs`, `pub(crate)`). It is resolved at the same known point as `SharedEngineServices` (realm install, `ensure_execution`), shut down at full loop-exit teardown, and reachable only by injection. There is no global accessor, no thread-local, and no way for a library crate to reach the pools. Realms and presentations will receive capability handles from it when #558 defines them; they do not resolve it themselves.

### Work classes are lanes, not a priority enum

| Class | Where it runs | API |
|---|---|---|
| Frame-required compute | The owner thread itself: the frame pipeline plus `AsyncDriver`'s mid-frame poll | none here — deliberately not spawnable onto a pool |
| Asynchronous compute | Bounded worker pool, `default_compute_worker_count` workers (`available_parallelism − IO_WORKER_THREADS − 1`, min 1), `flui-compute` threads, lazily started | `spawn_compute(ComputeJob)` |
| IO | Small fixed async runtime, 2 `flui-io` workers, lazily started | `spawn_io(IoFuture)` |
| Durable services | **Not this ADR** — issue #558's lifecycle work | — |

"Background work cannot starve frame-required compute" therefore has two halves, each pinned by a test: *structural* (the frame lane never runs on these pools, so pool saturation cannot block it — `frame_lane_makes_progress_while_background_lanes_are_saturated`) and *sizing* (the background lanes together leave the owner thread a hardware thread — `compute_pool_sizing_leaves_owner_thread_headroom`). There is no user-facing priority parameter; scheduling policy derives from the class chosen at the call site.

### Bounded admission, cancellation, deadline-bounded shutdown

Both lanes count in-flight work against a cap; a full lane refuses with `SpawnError::Saturated` (backpressure, not queue growth). Shutdown is staged, per the adoption guide's shape: **stop admission** (later spawns get `SpawnError::ShuttingDown`) → **cancel** (a `tokio_util::sync::CancellationToken` wrapper travels with every unit of work: queued compute jobs skip, IO futures resolve early and drop at their await point) → **join** running work, bounded by a per-pool grace deadline (`shutdown_timeout`; 5s at loop exit). The cancellation wrapper is load-bearing precisely for *host-injected* pools, whose tasks FLUI cannot drop any other way (`shutdown_cancels_work_handed_to_host_pools` fails without it). `ExecutionServices::drop` is the non-blocking last resort (`shutdown_background`), mirroring flui-assets' bridge-runtime discipline.

### Host injection avoids duplicate pools

`AppConfig::with_executors(HostExecutors)` carries two runtime-neutral trait objects (`HostComputePool`, `HostIoPool` — boxed-closure and boxed-future contracts, deliberately not Tokio types, per the adoption guide's "the contract is an injected executor and must remain runtime-neutral"). The bootstrap stashes them into `AppRuntime` *before* realm install resolves the services; with a host present the default pools are **never constructed** — pinned by `host_injection_routes_work_and_never_starts_default_pools`. FLUI's admission and cancellation wrappers apply identically on top of host pools, so the observable contract does not depend on who owns the threads.

### Determinism and wasm

`DeterministicExecutors` (public) is a single-threaded FIFO implementation of the same seam — work runs only inside `run_until_idle()`, on the calling thread, in spawn order. The admission/shutdown conformance tests run against both the default pools and this injected implementation. On wasm32 the same `ExecutionServices` API is sequential: compute runs inline at the spawn site, IO goes to the browser microtask queue via `wasm_bindgen_futures::spawn_local`, and shutdown stops admission only (queued microtasks cannot be revoked; there is nothing to join).

### The platform executor is defanged ahead of removal

`BackgroundExecutor` (already classified removal-target under #557 in `docs/runtime-contract.toml`) no longer claims a full-core pool per platform instance: construction starts zero threads (lazy `OnceLock`), and the pool that starts on first *use* is a small fixed size (2 workers). FLUI-managed runs never use it, so they never start it. Its deletion — together with `flui_platform::Task`/`Priority` — is the follow-up slice, not this one: those surfaces are only lint-gated on Windows/macOS and their internal consumers (prompt marshaling) deserve their own change.

## Alternatives considered

- **One shared runtime for both background classes.** Fewer threads, but a CPU-bound closure occupying an async worker starves IO futures — exactly the class-interference the issue exists to prevent. Two pools with a joint sizing budget keep the isolation and still bound total threads.
- **`rayon` for the compute pool.** Explicitly deferred by the adoption guide until the serial/parallel job-graph spike (#562) proves crossover thresholds. A Tokio multi-thread runtime used as a plain worker pool costs nothing extra here and avoids a new dependency with no proven consumer.
- **Per-realm (rather than loop-scoped) pools.** Worker threads are a machine-level resource; N realms with N pools recreates the oversubscription problem one level down. Realms get capability handles (#558), not pools.
- **A tokio `Handle` as the injection type.** Simplest, but freezes Tokio into the public embedding contract; the guide forbids it. The trait seam costs one `Box` per spawn on a path that is per-job, not per-frame.
- **Keeping `Priority` public with implemented semantics.** Rejected by the study: deadline/behavior classes, chosen at the call site, replace user-guessed priorities. `Priority`'s retirement rides the platform-surface removal slice.

## Consequences

- A managed desktop run now materializes **zero** background worker threads until the first background spawn, instead of `num_cpus` per platform instance at init.
- Issues #558 (Task/Worker/Service lifecycles, per-task handles, versioned results, owner-lifetime cancellation) and #559 (raster lane) build on these lanes; until they land the spawn lanes have no shipped production caller — an `expect(dead_code)` in `execution.rs` errors the moment one appears, forcing that attribute's removal. A breaking reshape of the seam when those consumers prove it wrong is expected and preferred over shims.
- Named residuals: flui-assets' bridged decode path still lazily starts its own single-worker runtime when no handle is injected (1 thread, not full-core; wiring it to the IO lane needs a flui-app → flui-assets edge decision — follow-up under #557); `Priority`/`Task`/`BackgroundExecutor` remain public removal-targets until the follow-up slice.
- `tokio-util` (default features, `CancellationToken` only) enters the workspace with this — its sanctioned first owning work per the adoption guide.
