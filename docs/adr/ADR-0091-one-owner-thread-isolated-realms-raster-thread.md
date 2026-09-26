# ADR-0091: One owner thread hosts isolated realms; one raster thread per GpuContext

- **Status:** Proposed
- **Date:** 2026-09-25
- **Amends (on acceptance):** [ADR-0027](ADR-0027-owner-affine-ui-realms.md) (the verdict and §1: realms are
  isolated, not concurrent, through H2; §5: one raster thread serves several presentations' raster
  owners), [ADR-0045](ADR-0045-raster-lane.md) (decision 2: one `GpuContext` per application
  instead of GPU services per owner thread, atlases single-owned on the raster thread; the
  acceptance criterion becomes per platform)
- **Related:** [ADR-0037](ADR-0037-presentation-ownership-domains.md),
  [ADR-0044](ADR-0044-driver-loop-hybrid.md), [ADR-0047](ADR-0047-unified-execution-services.md),
  [ADR-0058](ADR-0058-the-platform-paces-production-not-a-sleep.md),
  [ADR-0063](ADR-0063-the-renderer-owns-its-surface-target.md),
  [ADR-0067](ADR-0067-engine-owned-glyph-atlas.md),
  [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md),
  [ADR-0086](ADR-0086-signal-writes-through-event-context.md),
  [ADR-0087](ADR-0087-raster-contract-and-cpu-backend.md),
  [ADR-0092](ADR-0092-per-realm-text-over-parley.md),
  [ADR-0097](ADR-0097-no-process-global-state-gate.md)
- **Refs:** decisions D9 and D13 in the [decision index](../../design/decisions.md); target shape
  in [architecture](../../design/architecture.md)

## Context

[ADR-0027](ADR-0027-owner-affine-ui-realms.md)'s verdict says "Multiple realms may execute
concurrently", and its §1 lets "Win32, Linux and headless … use distinct owner threads". The code
has one owner thread. A single `thread_local!` `APP_RUNTIME` hosts every realm on desktop,
Android and wasm (`crates/flui-app/src/app/runner/host.rs:24-47`), and its own doc says why it
stays there: "The platform callback surface still requires `Send`, so the `!Send` realm this holds
remains in owner TLS". Those callbacks are `Box<dyn Fn() -> bool + Send>` and similar
(`crates/flui-platform/src/traits/platform.rs:319`). ADR-0027's Open questions call this
transitional, but the verdict states concurrency as fact.

What rules out parallel layout inside a realm is the pipeline's storage, not the render-object
types: `PipelineCell(Rc<RefCell<PipelineOwner>>)` (`crates/flui-rendering/src/pipeline/owner/cell.rs:51`).
The render-object bounds point the other way — `RenderView::RenderObject` must be `Send + Sync`
(`crates/flui-view/src/view/render.rs:451`) — although ADR-0027 §9 says render objects are not
`Send + Sync`.

On the raster side, [ADR-0045](ADR-0045-raster-lane.md) is still Proposed, and production runs
inline:

- Every runner builds one lane per window and wraps it in a lock:
  `Arc<Mutex<RasterLane>>` on desktop (`crates/flui-app/src/app/runner/desktop.rs:312`), Android
  (`runner/android.rs:269`) and iOS (`runner/ios.rs:322`). The lane pumps on the owner thread
  (`crates/flui-app/src/app/raster_lane.rs:1-13`).
- `RasterOwner::run_until_shutdown` (`crates/flui-engine/src/raster_owner.rs:1519`) has callers
  only inside that file's test module, which starts at `raster_owner.rs:1668`. No production
  raster thread exists.
- Web renders through `DirectSink`, the pre-mailbox path with no stamping or generation checks
  (`crates/flui-app/src/app/raster_lane.rs:457-463`, constructed at
  `crates/flui-app/src/app/ui_realm/frame.rs:478`), with the renderer in an
  `Arc<Mutex<Option<Renderer>>>` (`crates/flui-app/src/app/runner/web.rs:85`).
- Every renderer creates its own `wgpu::Instance`, surface and adapter
  (`crates/flui-engine/src/renderer.rs:1140-1168`), and every painter builds its own glyph atlas
  over the process-wide font system (`crates/flui-engine/src/painter/mod.rs:73`, `:163-167`).

ADR-0045 decision 2 scoped GPU services to one owner thread, shared only the `ShaderCache`, kept
the glyph atlas per surface, and rejected process-wide sharing because it "reintroduces a
process-global GPU resource". Its revisit condition was three or more surfaces with measured
duplicate atlas residency or pipeline compiles. The review found a second problem with per-window
threaded lanes: a glyph atlas shared across them needs a lock on the raster path, which AGENTS.md
forbids for per-frame state.

## Decision

### 1. Realms are isolated, not concurrent, through H2

One owner thread per process hosts every realm from H0 through H2. Realms keep everything
ADR-0027 gives them — their own scheduler, GlobalKey scope, focus, channels and shutdown — and
never share mutable state, but they take turns on that one thread. ADR-0027's verdict now reads:
"Each `UiRealm` has exactly one owner executor and performs its UI transaction serially. Realms
are isolated from each other; through H2 they share one owner thread." §1's per-platform
realm-to-thread sentence is replaced by this section.

Per-realm owner threads on Win32 and Linux are an H2 spike behind an `OwnerExecutor` trait. Its
success metric: with one window's realm blocked, another window stays within its frame budget.
Adopting them needs its own ADR, and needs the platform callbacks to stop requiring `Send`
([ADR-0082](ADR-0082-platform-api-contract-crate.md)) and the UI traits to become `!Send` as
ADR-0027 §9 already requires. That flip — `Listenable: Send + Sync` and `ListenerCallback`
(`crates/flui-foundation/src/notifier.rs:78`, `:46`), `RenderView::RenderObject`, and
`RenderObject::metadata()`'s `Arc<dyn Any + Send + Sync>`
(`crates/flui-rendering/src/traits/render_object.rs:661`) — is conformance work ADR-0027 already
demands, and this ADR schedules it: it lands **before the first crates.io publication** of any
FLUI crate, together with the callback signature change of
[ADR-0086](ADR-0086-signal-writes-through-event-context.md), because both change public bounds
and signatures, and a break after the first publication reaches every downstream crate. The
owner set this deadline on 2026-09-25; it replaces the earlier "no later than the H3 freeze".
It is earlier than per-realm owner threads could be adopted, so that older condition holds
automatically. Each callback family that loses `Send` gets its event context in the same change,
as ADR-0086 requires. No runtime behaviour before H2 depends on the flip; the publication does.

### 2. No parallel layout inside a realm

Parallel layout within one realm is not a goal, and the barrier is named: `PipelineCell` is
`Rc<RefCell<…>>`. ADR-0027 §10's four preconditions for revisiting it stand unchanged.

### 3. One `GpuContext` per application

`GpuContext` holds the instance, adapter, device, queue, `wgpu::PipelineCache`, the closed effect
catalogue with its pre-warm, and the atlases. There is one per application. It is owned by the
runtime host and handed to each presentation's raster owner at construction; it is never a
`static` and never reached ambiently ([ADR-0097](ADR-0097-no-process-global-state-gate.md) gates
that). This is what distinguishes it from the process-global resource ADR-0045 rejected: sharing
is a constructor decision, as ADR-0027 §1 already describes `SharedEngineServices`.

Surfaces are created from the context's instance, because an adapter resolves a compatible
surface against the instance that created it ([ADR-0045](ADR-0045-raster-lane.md) decision 2).
Adapter selection stays as ADR-0045 describes: the first window picks, for the process.

Device loss is a `GpuContext` event: every presentation's frames are rejected until the context
is rebuilt, so one device loss blanks every window for at least a frame (ADR-0045 already accepted
this per owner thread; there is now one owner thread).

### 4. One raster thread per `GpuContext`, atlases single-owned

One raster thread serves every presentation of a `GpuContext` through H2. The glyph atlas and the
image atlas live on that thread with exactly one owner and no lock. Each presentation keeps its
own raster owner state — its mailbox, its `SurfaceGeneration` counter (ADR-0045 decision 4), its
surface — so ADR-0027 §5's "bound to one presentation" still holds per raster owner; what changes
is that several raster owners share one thread. Per-window raster threads with per-window atlas
preparation and a merged upload are the alternative the H2 spike measures, with an atlas
contention counter as its metric.

### 5. Acceptance is per platform

ADR-0045's single criterion becomes a table. A platform is threaded when surface acquisition and
present run on the raster thread and the ADR-0045 pacing budget (median, p90 and max
inter-present interval against the display period) holds on that platform.

| Platform | Lane | Reason recorded |
|---|---|---|
| Win32 | threaded | — |
| Linux (winit) | threaded | — |
| macOS | inline | `wgpu-hal`'s Metal `acquire_texture` messages `NSWindow` from the acquiring thread; ADR-0045 decision 1 and its reopen condition (tracked as issue #653) |
| wasm | inline | no raster thread; web moves onto the lane protocol and `DirectSink` is deleted |
| Android, iOS | inline until measured | no platform reason is recorded against threading; the lane is threaded once a device run meets the budget |

### 6. `RasterOwner` stays in `flui-engine`

`RasterOwner` moves into the runtime only after [ADR-0087](ADR-0087-raster-contract-and-cpu-backend.md)
has moved `RasterBackend`, `PresentDisposition` and a wgpu-free `RasterError` into `flui-layer`,
so the runtime can hold it without reaching wgpu. The move is optional and not part of H0.

### 7. External content

External GPU content registers its textures on the `GpuContext` and is composited on the raster
thread. Because there is one queue, ordering between the producer's submission and the frame that
samples it is queue order; the fence contract for producers on other threads is written with the
first consumer (the external-content spike), not here.

## Alternatives considered

- **Per-realm owner threads now.** Rejected for H0–H2: the platform callbacks still require
  `Send`, so the `!Send` realm cannot leave the one TLS cell, and there is no measurement that a
  second owner thread helps a real workload. It remains the H2 spike.
- **Per-window raster threads with a shared atlas.** Rejected: the shared atlas needs a lock
  touched every frame. Per-window atlases avoid the lock but duplicate residency; the spike
  weighs that against isolation.
- **Accept ADR-0045 by a status change.** Rejected: its criterion is a single CI budget on the
  one platform CI runs, which would call macOS, web and mobile done by default.
- **GPU services per owner thread (ADR-0045 as written).** Superseded by this ADR's premise: with
  one owner thread, "per owner thread" and "per application" coincide, and naming it per
  application makes the ownership explicit instead of incidental to the thread count.

## Consequences

- ADR-0027's concurrency claim becomes true to the code; multi-window isolation is unchanged.
- One device and one atlas set serve every window: less memory and fewer pipeline compiles than
  per-window stacks, and one device loss affects every window.
- One raster thread presents several surfaces in turn. A present that blocks on one surface
  (`Fifo`, an occluded window) can delay another window's present; how much is unmeasured and is
  what the H2 spike and the per-platform budget measure.
- The lock around each lane (`Arc<Mutex<RasterLane>>`) goes away once the lane is threaded; the
  inline platforms keep an owner-thread lane with no lock once the callbacks stop requiring
  `Send`.
- Glyph rasterization moves to the raster thread, which needs a rasterizer that does not take the
  shaping lock; [ADR-0092](ADR-0092-per-realm-text-over-parley.md) provides it.
- [ADR-0067](ADR-0067-engine-owned-glyph-atlas.md)'s atlas moves from each painter to the
  `GpuContext`.

## Verification

None of these exist yet, apart from the threaded protocol harness in
`crates/flui-engine/src/raster_owner.rs`'s tests, which already spawns `run_until_shutdown` on a
thread.

- A two-window test that asserts one device and one glyph atlas are created.
- The process-global state gate of [ADR-0097](ADR-0097-no-process-global-state-gate.md) finds no
  `static` holding a `GpuContext`, a device or an atlas.
- A clippy `disallowed_types` entry or a type pin showing the atlas types hold no `Mutex`/`RwLock`.
- The per-platform pacing budget of §5, in CI for Linux, and a recorded run for Win32.
- The H2 spike's "one window blocked, the other within budget" test with its atlas contention
  counter.
