# Runtime Architecture Execution Plan

> Dependency-ordered work derived from the
> [UI Runtime Evolution Study](2026-08-01-ui-runtime-evolution-study.md).
> This is an execution plan for accepted ADR-0027/0037 direction and the
> validation of proposed ADR-0039. It does not redesign Flutter-loyal
> View/Element/Render behavior.

Candidate third-party crates, intended integration boundaries, and adoption
gates are maintained separately in the
[Runtime Dependency Adoption Guide](2026-08-01-runtime-dependency-adoption-guide.md).
Workspace-member responsibilities, facade composition, localization direction,
and logging ownership are reviewed in the
[Workspace Boundary and Logging Review](2026-08-01-workspace-boundary-and-logging-review.md).

**Date:** 2026-08-01
**Target:** desktop-first multi-window and hostable runtime foundations.
**Toolchain:** workspace-pinned stable Rust 1.97.1; the milestone does not
silently raise the declared `rust-version` floor.
**Compatibility:** breaking changes are expected before public contracts
graduate.

## Milestone outcome

At exit, FLUI can host several independently scheduled windows that share
application-owned models and engine services without sharing mutable UI trees.
The same runtime can be driven by FLUI or embedded in an external host. Raster
work consumes immutable presentation-addressed snapshots, frame pacing follows
the actual surface, background work cannot starve a frame, and shutdown/error
paths are deterministic.

The milestone does **not** promise mobile/web completion, a game engine, 3D,
parallel layout, or a stable public embedding ABI. It establishes the contracts
those later layers require.

## Dependency graph

```text
Evidence and API gate
        |
        +--> event-loop capability --> runtime singleton retirement
        |                                  |
        +--> presentation protocol --------+
                                           |
                                  multi-presentation forest
                                           |
        +--> executor/work classes --------+--> host-driven runtime
        |                                  |
        +--> frame-clock split ------------+--> threaded raster + latency
                                           |
        +--> transactional recovery --------+
                                           |
                                  conformance harness
                                           |
                                  adversarial reference app
                                           |
                                  stability graduation audit
```

## Pre-sprint structural gate

Before the runtime conformance matrix freezes transitional APIs, complete the
three preparation tasks defined by the workspace-boundary review:

1. [#567](https://github.com/vanyastaff/flui/issues/567): lock and mechanically
   check the real workspace topology;
2. [#568](https://github.com/vanyastaff/flui/issues/568): restore the
   cross-platform logging backend outside foundation, move process-global setup
   to composition roots, and prove embedded host inheritance;
3. [#569](https://github.com/vanyastaff/flui/issues/569): clean the public
   package surface, feature-select optional catalogs and development
   capabilities, and rename the headless test driver before its conformance API
   grows.

These tasks precede the work items below. They do not permit changes to the
Flutter-loyal three-tree behavior.

## Work items

### Establish a runtime conformance matrix and API freeze gate

**Problem.** Accepted ADRs describe target architecture, but there is no
executable inventory showing which invariants are implemented and which public
APIs expose transitional behavior.

**Solution.** Add a checked conformance matrix for ADR-0027, ADR-0037,
ADR-0029, and ADR-0039. Inventory public runtime/platform/scheduler types and
classify each as stable candidate, experimental, internal, or removal target.
Add compile-time and source gates for the invariants that can be checked
mechanically.

**Acceptance.** Every normative ADR clause has implementation evidence or an
owning issue; ignored/unwired public configuration is listed; no transitional
runtime type graduates accidentally.

### Make event-loop authority explicit

**Problem.** `Platform: Send + Sync` exposes methods that are owner-thread-only,
including window creation and several platform reads. Safety currently depends
on backend convention and runtime checks.

**Solution.** Validate and accept ADR-0039, then implement its `OwnerPlatform`
capability and typed cross-thread proxy. Move owner-affine operations out of the
shared `Platform` trait, retain direct same-owner operations, and marshal only a
closed command vocabulary. Validate Android bootstrap on-device before deleting
the old trait methods.

**Acceptance.** Wrong-thread window operations are compile errors; all desktop
backends use the same bounded lane contract; no generic closure executor is
public; native callback registration has explicit cancellation ownership.

**Depends on:** conformance matrix.

### Complete presentation identity and protocol addressing

**Problem.** `PresentationId` exists, but `SceneSnapshot` lacks it and several
routes still select the current/single presentation implicitly.

**Solution.** Make the runtime's window map the only
`WindowId -> (RealmId, PresentationId)` authority. Stamp platform-to-UI,
UI-to-raster, raster acknowledgements, semantics actions, input, focus, and
text-input traffic with the exact presentation incarnation. Add
`PresentationId` to `SceneSnapshot` and reject stale identities at every owner.

**Acceptance.** Recycled windows/presentations cannot accept late input,
semantics actions, worker results, or raster acks; no second native-window map
or current-window selector exists.

**Depends on:** conformance matrix; coordinates with event-loop capability.

### Retire binding singletons into explicit runtime ownership

**Problem.** `Scheduler`, rendering, painting, semantics, and `AppBinding`
remain ambient singleton graphs. They force the process-wide realm guard and
make isolation claims false.

**Solution.** Introduce private `AppRuntime` composition ownership. Move shared
engine services to explicit constructor-injected state; move update scheduling,
widget ownership, navigation, and lifecycle authority into the owning realm;
move presentation-specific focus/input/frame state into `PresentationState`.
Delete `HasInstance` use from runtime paths and remove the leaked thread-local
`AppBinding`.

**Acceptance.** Two realms can coexist without test locks or shared binding
state; dropping one cannot change the other; `REALM_CLAIMED` and
`UiRealmError::AlreadyExists` are gone; tests run in parallel.

**Depends on:** event-loop capability and presentation addressing.

### Replace lock-shaped UI ownership with direct owner-local access

**Problem.** `Arc<RwLock<PipelineOwner>>` is carried by presentation, view,
element, render tree, and bindings even though the accepted model has one
logical writer. It imposes `Send + Sync` bounds and enables unsafe raw-pointer
traits that runtime checks must defend.

**Solution.** Store `PipelineOwner` directly in the presentation owner domain.
Use owner-lifetime leases/capabilities or IDs for synchronous access and typed
handles for cross-thread dirty/result delivery. Remove UI-tree `Send + Sync`
bounds where production does not move those objects across threads. Retire
`NodePtr` unsafe thread traits after the subtree arena has a single-owner borrow
model.

**Acceptance.** No `Arc<RwLock<PipelineOwner>>` appears in production ownership
paths; `RenderObject` does not pay a false thread-safety tax; Miri and compile
tests pin the actual boundary; layout/build remain behaviorally identical.

**Depends on:** runtime singleton retirement.

### Reify a realm-owned presentation forest and multi-window policy

**Problem.** `UiRealm` stores exactly one `PresentationState` and one element
root. Multi-window semantics therefore cannot be implemented without aliases or
special cases.

**Solution.** Generalize `BuildOwner` to an element forest with independent
presentation roots. Give each presentation its own pipeline, focus, gestures,
mouse tracking, text input, semantics, and restoration state. Keep GlobalKey
identity realm-scoped. Default desktop policy creates one realm per independent
window; one realm with several presentations is available for shared-session
use cases only after forest invariants are proven.

**Acceptance.** Independent windows can run concurrently and share explicit
application models; a one-realm/two-presentation test proves root isolation;
closing or rebuilding one root cannot affect another; focus and semantics route
to the exact presentation.

**Depends on:** direct owner-local pipeline and complete presentation identity.

### Split logical updates, presentation clocks, and raster scheduling

**Problem.** The singleton `Scheduler` combines logical update phases, a 60 FPS
budget, animation callbacks, lifecycle, and frame pacing. `AppConfig` exposes
target FPS and vsync fields that do not govern the actual present path.

**Solution.** Split realm-owned `UpdateScheduler`, presentation-owned
`FrameClock`, and raster backpressure. Derive deadlines from platform display
timing and visibility. Coalesce frame demand from dirty state, animation, media,
and external host requests. Default to automatic frame pacing; keep optional
target-rate and max-frames-in-flight controls at the advanced renderer boundary.
Remove or wire every misleading `AppConfig` field.

**Acceptance.** Two windows on different refresh rates are paced independently;
hidden/minimized surfaces stop GPU work without freezing wall-clock services;
60 FPS is no longer a global default contract; public configuration is fully
effective and tested or removed.

**Depends on:** runtime singleton retirement and presentation forest.

### Unify worker, I/O, and service execution with host injection

**Problem.** Each platform constructs a full-size Tokio runtime; priority is
informational; there is no host executor injection, workload isolation, or
uniform cancellation/backpressure contract.

**Solution.** Move execution services under `AppRuntime`. Provide separate
work classes for frame-required pure compute, asynchronous compute that may
span frames, I/O, and durable application services. Use bounded admission,
cancellation tokens, task tracking, and versioned results. Permit an embedded
host to provide compatible executors so FLUI and a game engine do not
oversubscribe the machine.

**Acceptance.** One runtime owns the default pools; background work cannot
starve frame-critical jobs; priority has observable tested behavior or is not
public; shutdown joins/cancels tracked work; an injected deterministic executor
passes the same tests.

**Depends on:** conformance matrix; integrates after runtime ownership exists.

### Define durable service lifecycle and graceful application shutdown

**Problem.** Ordinary tasks, long-running workers, OS-backed background
services, and process-isolated work have different lifecycle and reliability
requirements but no unified application policy.

**Solution.** Keep distinct concepts: scoped `Task`, reusable `Worker`, durable
`Service`, and optional `ProcessWorker`. Services declare whether they stop with
the last window or keep the app alive. Shutdown follows request, cancel/defer,
cancellation, deadline, flush, and forceable termination stages. Persist
recoverable state continuously rather than trusting the final shutdown window.

**Acceptance.** Messenger-like background operation and editor-like exit both
work without framework special cases; close can be cancelled or deferred;
late service results cannot resurrect a closed realm; shutdown never waits on a
possibly-full ordinary queue.

**Depends on:** unified execution services and runtime ownership.

### Move raster ownership to a real lane and implement latency policy

**Problem.** The mailbox protocol is threaded-testable, but the shipping raster
owner is synchronous. GPU service sharing, multiple surfaces, device loss, and
frames-in-flight policy are not yet implemented end to end.

**Solution.** Add the deferred raster-thread ADR. Share device/queue and
generation-aware caches at `AppRuntime`; keep surfaces, configuration, frame
state, and presentation timing per raster owner. Preserve the capacity-one
latest-frame mailbox, explicit acks, and deterministic command order. Implement
automatic pipeline/non-pipeline selection where supported and an advanced
`max_frames_in_flight` limit.

**Acceptance.** UI and raster overlap measurably without accessing each other's
mutable objects; device loss recreates resources without stale publication;
multi-surface resize/close is race-free; latency and frame-time distributions
are captured, not inferred from average FPS.

**Depends on:** presentation addressing, frame-clock split, and shared runtime
services.

### Add an experimental host-driven runtime and renderer interop

**Problem.** `run_app` owns the whole loop. Games, editors, and existing engines
need to drive updates, inject input, share GPU services, and choose a target
without wrapping FLUI in a widget/plugin adapter.

**Solution.** Keep `run_app` as the normal facade and add an explicitly
experimental host-driven runtime surface. It accepts host timing/input, pumps
bounded runtime work, and renders to a surface, texture, or external target.
Where technically valid, accept a host `wgpu::Device`/`Queue`; never silently
create a second device. Make the API engine-neutral and validate it with a
minimal Bevy or custom-loop adapter outside core.

**Acceptance.** The same UI tree passes managed-loop and host-loop conformance
tests; no fixed simulation clock enters core; host and FLUI share one GPU device;
input, resize, suspend, resume, and shutdown have deterministic ordering.

**Depends on:** runtime ownership, execution injection, frame-clock split, and
raster lane.

### Make frame failure recovery transactional

**Problem.** Catching a panic after partial layout mutation and substituting a
zero result can leave the tree degraded while reporting that the frame
recovered.

**Solution.** Classify failures as caller validation errors, recoverable subtree
failures, backend failures, and internal invariant violations. Validate caller
input before frame entry. Build recoverable presentation output into a
transactional candidate and publish only after validation; retain the last good
snapshot on failure. Development mode renders a detailed local error view;
production keeps a neutral local fallback and emits structured diagnostics.

**Acceptance.** A failed subtree cannot partially commit layout, hit-test, paint,
or semantics state; another window continues; production diagnostics contain no
sensitive payload by default; invariant bugs remain loud under the panic policy.

**Depends on:** direct owner-local pipeline and presentation commit boundary.

### Establish deterministic serial/parallel execution gates

**Problem.** The stale parallel-layout issue proposes locks and Rayon without a
dependency model, serial oracle, cost threshold, cancellation, or proof that
output order remains stable.

**Solution.** Define one internal job graph runnable by a deterministic serial
executor and a parallel executor. Start with pure expensive edges: decode,
shaping where isolation permits, tessellation, resource preparation, and later
large independent repaint boundaries. Every job declares immutable inputs,
version, cancellation, and commit owner. Parallel layout remains gated until
dependency extraction, font/cache sharding, snapshot commit, deterministic
ordering, and benchmarks all pass.

**Acceptance.** Serial and parallel runs produce identical display lists,
semantics, hit-test order, and diagnostics; deterministic replay reproduces a
frame; benchmarks define crossover thresholds; disabling parallelism remains a
supported diagnostic mode.

**Depends on:** unified executors, owner-local pipeline, and transactional
commit.

### Build multi-window and platform conformance infrastructure

**Problem.** Headless tests do not exercise native event-loop restrictions;
Win32/AppKit CI only type-checks; `flui-platform` tests are excluded because of
an unresolved heap-corruption failure.

**Solution.** Resolve the platform-test corruption first. Add a deterministic
headless multi-realm/multi-presentation harness, threaded protocol tests without
sleeps, native backend smoke tests, and trace replay. Test cross-window focus,
IME, DnD, semantics, DPI/refresh migration, surface loss, activation, and close.

**Acceptance.** The platform suite runs in CI; every native backend has at least
one executing smoke lane or a documented external hardware lane; protocol tests
cover late events and shutdown races; test isolation needs no process-global
locks.

**Depends on:** all ownership/protocol work; individual harness pieces can land
alongside their owning changes.

### Validate with an adversarial multi-window reference application

**Problem.** Counters and isolated examples do not expose lifetime, contention,
restoration, embedding, or service problems.

**Solution.** Build the editor-shaped proof described in the research study:
shared documents/configuration, independent windows, background indexing,
embedded realtime wgpu viewport, restoration, unsaved-close deferral,
accessibility, and deterministic replay. Keep product-specific features in the
example, not framework core.

**Acceptance.** The app runs through managed and host-driven entry points;
opening/closing windows under worker and GPU load is race-free; no window blocks
another's UI transaction; forced termination recovers journaled state; profiling
captures p50/p95/p99 phase and input-to-present latency.

**Depends on:** multi-presentation, services, raster lane, embedding, recovery,
and conformance infrastructure.

### Graduate only proven public APIs

**Problem.** Transitional or speculative APIs can become permanent contracts
before real use exposes their wrong abstraction level.

**Solution.** Run a final public-surface review after the reference app. Keep
composition roots and protocol machinery private. Stabilize conventional
author-facing names only where both managed and embedded consumers agree.
Publish migration notes for every removed pre-1.0 API and record remaining
tradeoffs in ADRs.

**Acceptance.** No public lock guards, ambient singleton accessors, ignored
configuration, opaque payloads, or generic UI executors remain; every stable
runtime API has two consumers, failure semantics, thread-affinity docs, and
conformance tests.

**Depends on:** reference application and full milestone verification.

## Work intentionally outside this milestone

- `flui-game`, ECS, physics, simulation clocks, and fixed timesteps.
- `flui-3d` and a first-party 3D scene graph.
- Mobile and web product completion beyond compile/protocol preservation.
- Public plugin ABI or untrusted extension sandbox.
- General parallel layout.
- Automatic quality degradation based on guessed frame-budget heuristics.

These become separate milestones only after a concrete consumer and benchmark
justify them.

## Verification gate

- All accepted/proposed runtime ADR clauses have evidence.
- `just ci`, `taplo fmt --check`, and `typos` pass.
- Miri covers owner-local traversal and generation rejection paths.
- Loom or an equivalent controlled scheduler covers mailbox/shutdown races.
- Every new runtime dependency satisfies the dependency-adoption checklist and
  has a current milestone consumer.
- Native platform evidence is reported separately from headless evidence.
- Performance reports include p50/p95/p99 and missed-deadline counts, not only
  average FPS.
- Managed and embedded execution produce equivalent output for the same replay.
- The roadmap describes implemented behavior, not target architecture as fact.
