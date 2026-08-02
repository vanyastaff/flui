# Runtime Dependency Adoption Guide

> Candidate crates for the Runtime.1 architecture, how FLUI would use them,
> and the evidence required before they become workspace dependencies.

**Date:** 2026-08-01
**Status:** research and adoption policy; this document does not add dependencies.
**Toolchain constraint:** workspace stable Rust 1.97.1 and declared
`rust-version = "1.97"` remain unchanged.
**Related work:** [Runtime Architecture Execution Plan](2026-08-01-runtime-architecture-execution-plan.md),
[UI Runtime Evolution Study](2026-08-01-ui-runtime-evolution-study.md), and
[Rust Accessibility Ecosystem](2026-06-09-rust-a11y-ecosystem.md).

## Decision rule

A popular crate is not automatically a useful framework dependency. FLUI adds
one only when all of the following are true:

1. A current milestone task has a production or test consumer.
2. The crate removes difficult, non-product-specific machinery rather than
   defining FLUI's public architecture.
3. Its ownership, cancellation, backpressure, thread-affinity, and failure
   semantics match the relevant FLUI contract.
4. Its MSRV, target matrix, license, maintenance, unsafe surface, compile-time
   cost, and transitive dependencies have been checked.
5. The dependency is introduced with minimal features and a test that would
   fail without the behavior it supplies.
6. FLUI can replace it behind a private adapter if the crate changes direction.

Being present in `Cargo.lock` transitively is not a reason to expose or depend
on a crate directly. A direct workspace dependency is added with its first real
consumer, not in anticipation of one.

## Recommended adoption order

| Priority | Crate | Decision | First owning work |
|---|---|---|---|
| 1 | `loom` | Add as a narrowly scoped dev-dependency | mailbox, shutdown, and stale-generation protocol tests |
| 2 | `tokio-util` | Add with the runtime task/service implementation | cancellation and graceful shutdown |
| 3 | `accesskit` plus native adapters | Add with the first complete desktop semantics bridge | multi-window accessibility |
| 4 | `objc2` family | Migrate the AppKit backend in a dedicated change | modern Apple platform bindings |
| 5 | `rayon` | Adopt only after the serial/parallel job-graph spike passes | owned CPU compute pool |
| 6 | `hdrhistogram` | Add to devtools after a measurement-overhead benchmark | latency distributions |

## Adopt with Runtime.1

### `tokio-util`: structured cancellation and task draining

**Why it is useful.** `CancellationToken` models a cloneable cancellation tree,
while `TaskTracker` lets the runtime wait until tracked tasks have exited. This
matches the planned distinction between a task, worker, and durable service
without making Tokio itself part of FLUI's author-facing model.

**Where it belongs.** A private default-executor implementation owned by
`AppRuntime`. The public boundary should express FLUI capabilities such as
spawn, cancellation, deadline, and completion; it should not return Tokio
handles.

**Illustrative internal shape, not a public API commitment:**

```rust,ignore
struct RuntimeTasks {
    accepting_work: AtomicBool,
    cancel: CancellationToken,
    tracked: TaskTracker,
}

async fn shutdown(tasks: &RuntimeTasks, deadline: Instant) {
    tasks.accepting_work.store(false, Ordering::Release);
    tasks.cancel.cancel();
    tasks.tracked.close();
    wait_until(deadline, tasks.tracked.wait()).await;
}
```

Stopping admission before calling `TaskTracker::close` is important: closing a
tracker allows `wait` to complete but does not itself prohibit new spawns. FLUI's
wrapper owns that stronger invariant. Child tokens should follow realm,
presentation, worker, and service lifetimes. Results still carry generation
identity because cancellation is cooperative and can race with completion.

**Add when:** the runtime-owned executor and service shutdown task begins.

**Acceptance evidence:** cancellation propagation, bounded shutdown deadline,
late-result rejection, no task admission after shutdown begins, and equivalent
tests for the injected deterministic executor.

### `loom`: concurrency protocol verification

**Why it is useful.** Ordinary stress tests rarely reproduce the exact ordering
that loses a wake-up, accepts a result after close, or deadlocks shutdown. Loom
systematically explores valid interleavings for small concurrent models.

**Where it belongs.** Dev-dependencies of the crate that owns the protocol. It
must not leak into production types or be used to model the whole UI framework.

```rust,ignore
#[test]
fn close_wins_over_late_frame_ack() {
    loom::model(|| {
        let state = Arc::new(ProtocolState::new());
        let closing = spawn_close(state.clone());
        let late_ack = spawn_ack(state.clone(), old_generation());

        closing.join().expect("close thread");
        late_ack.join().expect("ack thread");
        assert!(state.has_no_live_old_generation());
    });
}
```

Production protocol code should use a tiny synchronization facade where needed
so tests substitute Loom atomics, mutexes, and threads. Keep models bounded to a
few actors and states to avoid combinatorial explosion.

**Add when:** the first Runtime.1 mailbox or shutdown state machine is changed.

**Acceptance evidence:** tests cover close versus send, cancel versus complete,
queue-full shutdown, recycled presentation identity, and wake-up publication.

### `accesskit`: native accessibility bridge

**Why it is useful.** AccessKit provides a cross-platform accessibility
interchange tree and maintained native adapters. Its atomic `TreeUpdate` model
fits FLUI's retained semantics tree and presentation-addressed snapshot model.

**Where it belongs.** FLUI semantics remain the behavioral source of truth.
Each native presentation translates a committed `SemanticsSnapshot` into an
AccessKit update on the platform owner thread. AccessKit IDs must be derived
from stable FLUI semantics identity plus the presentation incarnation; adapter
objects are per native window, never process-global.

```rust,ignore
fn publish_semantics(
    presentation: PresentationId,
    snapshot: &SemanticsSnapshot,
    adapter: &mut PlatformAccessibilityAdapter,
) {
    let update = translate_to_accesskit(presentation, snapshot);
    adapter.update_if_active(update);
}
```

The translation layer must preserve focus, actions, text selection, transforms,
live regions, and stale-action rejection. Use platform-specific adapters for
the native backends; `accesskit_winit` is appropriate only for the fallback
winit backend. Web still needs a separate DOM/ARIA bridge.

**Add when:** a complete desktop adapter is implemented, beginning with one
platform and a shared translation test corpus.

**Acceptance evidence:** external platform inspectors see the correct tree;
actions route to the exact presentation; closed-window actions are rejected;
focus, editable text, and multi-window isolation have native tests.

## Adopt after a measured spike

### `rayon`: owned pool for pure CPU jobs

**Why it may be useful.** Work stealing and scoped fork-join execution are a
good fit for coarse, independent CPU work: image decode, tessellation, resource
preparation, and text shaping where cache ownership permits it.

**Required integration.** Build a runtime-owned `rayon::ThreadPool` with an
explicit thread count and names. Never call `build_global`, and never invoke
top-level parallel iterators outside the owned pool. The host-driven runtime
must be able to replace or disable this pool to avoid oversubscription.

```rust,ignore
let compute = rayon::ThreadPoolBuilder::new()
    .num_threads(policy.compute_threads())
    .thread_name(|index| format!("flui-compute-{index}"))
    .build()?;

let result = compute.install(|| execute_pure_job_graph(input));
```

Jobs accept immutable inputs and carry a presentation/realm generation. Only
the owner thread commits a result. The same job graph must run through a serial
executor as the correctness oracle.

**Do not add yet.** First prove useful crossover thresholds and deterministic
serial/parallel equivalence. Rayon must not be used as justification for
parallel widget build, layout, or paint.

### `hdrhistogram`: tail-latency evidence

**Why it may be useful.** Average FPS hides the stalls users feel. A bounded HDR
histogram can report p50, p95, p99, and maximum input-to-present, build, layout,
paint, raster, and queue-wait latency across a wide range.

**Required integration.** Record integer nanoseconds or microseconds into
per-lane histograms, merge outside hot phases, and expose snapshots through
devtools. The measurement design must account for coordinated omission rather
than treating missing samples during a stall as success.

**Add when:** the Runtime.1 latency schema is fixed and benchmarked against a
minimal in-house histogram. Keep it out of author-facing crates.

### `tracing-tracy`: optional interactive profiling

**Why it may be useful.** FLUI already emits structured `tracing` spans. An
optional Tracy layer can visualize cross-thread scheduler, worker, and raster
activity with little integration code.

**Constraints.** Enable only through an explicit development feature. Tracy is
a profiler rather than the durable diagnostics format; its representation does
not perfectly preserve spans that move between threads, and some configurations
advertise a profiler endpoint on the local network. Chrome trace export remains
the portable baseline.

## Separate platform and rendering investigations

### `objc2`, `objc2-app-kit`, and `objc2-foundation`

The native macOS backend currently directly depends on the older `cocoa` and
`objc` crates. The typed `objc2` ecosystem is the preferred migration target
for AppKit and later UIKit work. This should be a replacement project, not a
permanent mixture of two binding styles in the same backend.

The migration must preserve main-thread rules, Objective-C object ownership,
autorelease-pool boundaries, callback lifetime, and native window behavior.
Cross-compilation proves only that code type-checks; AppKit smoke and lifecycle
tests on macOS are required before removing the old bindings.

### `parley`: rich text candidate

Parley provides shaping, bidi reordering, line breaking, alignment, editing
primitives, inline boxes, and optional AccessKit integration. It is worth a
comparison against the current cosmic-text/glyphon path, especially for rich
text and editor use cases.

Do not switch based on API breadth. Build a corpus covering Flutter text layout,
mixed-direction text, grapheme navigation, selection, IME composition, fallback
fonts, variable fonts, accessibility bounds, and repeated relayout. Compare
correctness, allocations, cold-start cost, and frame latency.

### `usvg` and `resvg`: SVG asset pipeline

`usvg` is useful as a parser and normalizer for untrusted, complex SVG input;
`resvg` provides a mature CPU raster fallback. They belong in the asset/image
pipeline behind FLUI types, not in the render-tree contract. Evaluate parse
limits, external resource policy, font resolution, cache keys, and raster scale
before adoption.

### `vello`: experimental raster backend only

Vello is a serious GPU 2D renderer, but it requires compute support and deep
`wgpu` integration. Preserve the existing `RasterBackend` seam and test Vello
as an optional backend against the same scene/readback corpus. Adoption requires
better results on representative FLUI scenes, a fallback path, device-loss
behavior, and no weakening of deterministic output. It is not a Runtime.1 core
dependency.

## Explicit non-adoptions

| Candidate | Decision | Reason |
|---|---|---|
| `flume` or `kanal` | Do not replace `crossbeam-channel` now | FLUI's missing contract is bounded admission, ownership, cancellation, and stale-result handling, not another queue implementation |
| `slotmap` or `generational-arena` | Do not adopt for core IDs | the workspace already has a consistent Slab plus one-based `NonZeroUsize` identity contract |
| `arc-swap` | Do not use as ambient runtime state | immutable snapshots should transfer through an explicit bounded protocol, not become globally readable mutable authority |
| Bevy ECS or task runtime | Keep outside core | it would impose an engine architecture on ordinary applications and complicate embedding in other hosts |
| another async runtime | Do not add | the default implementation may use Tokio, but FLUI's contract is an injected executor and must remain runtime-neutral |
| `mimalloc` | Profile first | allocator replacement can hide ownership/allocation problems and changes process-wide behavior |
| `core_affinity` or `thread-priority` | Profile and validate per OS first | scheduling hints are platform-specific and must have measurable semantics before becoming configuration |
| `rkyv` | Do not use for initial replay formats | versioning and validation costs are unnecessary until JSON/Serde evidence shows serialization is a bottleneck |

## Dependency introduction checklist

Every dependency PR should record:

- the owning milestone issue and first source consumer;
- exact enabled and disabled features;
- MSRV and desktop/mobile/web target results;
- license and advisory review;
- direct and significant transitive dependencies;
- unsafe-code inventory or isolation boundary;
- binary-size, clean-build, and hot-path impact where relevant;
- cancellation, panic, thread-affinity, and shutdown semantics;
- replacement boundary and why the crate is not exposed publicly;
- tests and benchmarks that justify adoption.

Run the workspace gates plus feature-matrix and target-specific checks. A crate
that cannot support a target must be target-gated at the narrowest owning crate,
not allowed to remove that target from unrelated framework layers.

## Sources checked

- Tokio Util `TaskTracker` and `CancellationToken` documentation:
  <https://docs.rs/tokio-util/latest/tokio_util/>
- Loom concurrency model and limitations:
  <https://docs.rs/loom/latest/loom/>
- AccessKit core and native adapters:
  <https://docs.rs/accesskit/latest/accesskit/>
- Rayon owned thread pools:
  <https://docs.rs/rayon/latest/rayon/struct.ThreadPoolBuilder.html>
- objc2 AppKit bindings:
  <https://docs.rs/objc2-app-kit/latest/objc2_app_kit/>
- HDR Histogram:
  <https://docs.rs/hdrhistogram/latest/hdrhistogram/>
- tracing-tracy caveats and features:
  <https://docs.rs/tracing-tracy/latest/tracing_tracy/>
- Parley text layout:
  <https://docs.rs/parley/latest/parley/>
- usvg and resvg:
  <https://docs.rs/usvg/latest/usvg/> and <https://docs.rs/resvg/latest/resvg/>
- Vello renderer requirements:
  <https://docs.rs/vello/latest/vello/>
