# ADR-0305: One GPU context serves every surface through a raster executor

- **Status:** Accepted (2026-11-03)
- **Date:** 2026-11-03
- **Supersedes:**
  - [ADR-0045](ADR-0045-raster-lane.md) decisions 1 to 7, as the table in More information maps
    each: what stands is restated here as accepted, and the rest is replaced (one value crossing
    down and `RasterBackend: Send`; `GpuServices`, `ReplaceServices` and owner-built replacement
    surfaces; the GPU-resource axis; `PipelineDepth` and `RasterMode`; health per lane).
  - [ADR-0087](ADR-0087-raster-contract-and-cpu-backend.md) §1, §2 and §5 only (the raster
    contract's move to `flui-layer`, the CPU backend with its conformance suite, and their timing).
  - [ADR-0091](ADR-0091-one-owner-thread-isolated-realms-raster-thread.md) §6 only (the optional
    move of `RasterOwner` into the runtime once ADR-0087 §1 lands).
  - [ADR-0063](ADR-0063-the-renderer-owns-its-surface-target.md) decision 1's constructor and
    decision 6's renderer methods only (`Renderer::new(target)`, `Renderer::release_surface` and
    `Renderer::recreate_surface` as the window's entry points).
  - [ADR-0058](ADR-0058-the-platform-paces-production-not-a-sleep.md) decision 1's delivery only
    (the pre-present hook installed through `RasterBackend::set_pre_present_hook`).
- **Amends:** [ADR-0091](ADR-0091-one-owner-thread-isolated-realms-raster-thread.md) §3, §4 and §7
  (their shape: the executor owns the context, and a slot per presentation holds what §4 calls
  its raster owner state and §7's external textures);
  [ADR-0254](ADR-0254-window-contract.md) decision 5 (the target goes to the owner's
  `SurfaceFactory`, and the instance's display is the platform's connection);
  [ADR-0267](ADR-0267-a-lost-gpu-device-is-rebuilt-on-a-backoff-and-given-up.md) (the rebuild is
  the context's, once per context); [ADR-0067](ADR-0067-engine-owned-glyph-atlas.md) (one atlas
  per context, aged by queue submission); [ADR-0257](ADR-0257-frame-timing-and-latency.md)
  decision 4 (the per-window frame latency is the slot's setting)
- **Related:** [ADR-0304](ADR-0304-the-scene-is-retained-and-damage-is-the-footprint-of-its-patch.md)
  (the scene state a frame carries, which a coalesced frame folds);
  [ADR-0244](ADR-0244-os-version-floors-and-gates.md) decision 1 (the backend probe, run once);
  [ADR-0100](ADR-0100-prepared-gpu-work-and-retained-frame-commit.md) (the device domain and the
  committed image); [ADR-0258](ADR-0258-owner-turn-panic-boundary-and-terminal-exit.md) §1 and §6
  (a contained panic's report and the dropped-frame backoff);
  [ADR-0287](ADR-0287-owner-state-is-committed-before-user-code-runs.md) and
  [ADR-0281](ADR-0281-a-destructor-that-panics-during-an-unwind-aborts-as-in-std.md) (commit, then
  call; the first failure kept); [ADR-0266](ADR-0266-one-owner-thread-ui-runtimes-isolate.md) (one
  owner thread); [ADR-0068](ADR-0068-frame-disposition-replaces-the-presented-bit.md) §2 (closed
  enums); [ADR-0097](ADR-0097-no-process-global-state-gate.md) (no process-global GPU state);
  [ADR-0136](ADR-0136-owner-local-ui-surfaces.md) (the `Send` positions);
  [ADR-0233](ADR-0233-external-texture-registry.md) (external textures per presentation);
  [ADR-0037](ADR-0037-presentation-ownership-domains.md) §8 and §11 (the generation authority;
  several presentations in one runtime)
- **Sources:** `gfx-rs/wgpu@40f4a34eba` (wgpu 30.0.1, the version `Cargo.lock` holds); the crate
  `linebender_resource_handle` 0.1.1 as `Cargo.lock` holds it

## Context and problem statement

[ADR-0091](ADR-0091-one-owner-thread-isolated-realms-raster-thread.md) §3 and §4 decided one
`GpuContext` per application and one raster thread per context, and left their shape open.
[ADR-0045](ADR-0045-raster-lane.md), still Proposed, designed GPU services per owner thread,
recovered by a `ReplaceServices` command; [ADR-0087](ADR-0087-raster-contract-and-cpu-backend.md)
§1 and §2, also Proposed, a raster contract in `flui-layer` with a second, CPU backend. The code
has neither. Evidence is at `e5ba8fe8f`.

| Fact | Evidence |
|---|---|
| Each window builds a private instance, adapter, device and queue, its own pipeline set and its own glyph atlas | `crates/flui-engine/src/renderer.rs:836-890`, `:995-1009`; `crates/flui-engine/src/painter/mod.rs:213`, `:221` |
| The instance takes its display from the first window's target; wgpu keeps it "for keepalive and validation", queries it with `expect` at every `create_surface`, and refuses a surface from another display | `renderer.rs:1550-1558`; `crates/flui-engine/src/window_target.rs:50-55`; `wgpu-core/src/instance.rs:56-62`, `:205-215` |
| A retired window's target answers `Unavailable` from then on, so an instance holding it panics at the next surface it creates | `crates/flui-platform/src/surface_target.rs:28-56`; `crates/flui-native/src/window/linux/wayland.rs:2547-2575`; `crates/flui-native/src/window/linux/x11/window.rs:1180-1230` |
| A surface is made only on the main thread (Apple) or the looper thread (Android) | `surface_target.rs:18-25` |
| The lane sits behind `Arc<Mutex<_>>`, justified by callbacks that "require `Send` closures"; the registrations take none, and the frame driver beside it already holds a `std::rc::Weak` | `desktop.rs:245-252`, `:419-425`; `crates/flui-app/src/app/raster_lane.rs:72-81`; `crates/flui-app/src/app/runner/window_routes.rs:185-237`; `crates/flui-app/src/app/runner/window_install.rs:269` |
| Eight production calls reach past the raster protocol into the backend | `crates/flui-engine/src/raster_owner.rs:1258` and the table in §12 |
| The GPU-resource axis of a frame stamp is `ZERO` on both sides, and its rejection is unreachable | `raster_lane.rs:344-348`, `:398-407` |
| A presented `Fifo` frame on the Vulkan and Wayland path blocks its thread until vsync (Windows is unmeasured), and a device rebuild blocks on `pollster` — today on the one owner thread every runtime shares | `renderer.rs:1751-1760`; `crates/flui-app/src/app/runner/device_recovery.rs:33`; ADR-0266 |
| wgpu's acquire timeout is a fixed second, and Metal ignores it | `wgpu-core/src/present.rs:32`, `:178`; `wgpu-hal/src/metal/surface.rs:349` |
| CI rasterises on Mesa's lavapipe (Vulkan) and llvmpipe (GL) | `.github/workflows/ci.yml:388-394`, `:430-436`; `tools/xtask/src/tasks.rs:598-626` |

What owns the GPU, how does each presentation reach it, on which thread does the work run, and
what does each failure cost?

## Decision drivers

- One device, one pipeline per key and format, and one glyph atlas serve every window
  (ADR-0091 §3).
- The owner thread never waits on raster work, except a bounded teardown wait (ADR-0045
  decisions 1 and 7); with one owner thread for every runtime (ADR-0266), a GPU wait there costs
  every window's input.
- A surface is made on the owner thread, and that is a type, not a comment.
- No lock and no `Send` the compiler did not ask for (ADR-0136; AGENTS.md, "The frame path is
  synchronous").
- Accepted work is delivered or refused with a typed error on every exit, and an answer is matched
  to its frame by stamp (AGENTS.md, "Recurring defects").
- A panic or a device loss costs what it touched: one slot's frame, the shared caches it may have
  torn, never the other windows' committed images.
- CI executes Linux and wasm32 only, and its GPU suites run on software Vulkan and GL.

## Considered options

1. A GPU stack per window, as today.
2. One `GpuContext`, owned by one raster executor that serves each presentation as a slot fed by
   commands; the executor runs inline or on a raster thread, per platform.
3. One `GpuContext` shared by a raster thread per window, its caches behind locks.
4. Option 2, inline on every platform, with a raster thread only once a measurement asks for one.
5. A raster contract in `flui-layer` with a wgpu and a CPU backend (ADR-0087 §1 and §2).

## Decision

Chosen option: 2, because it is the only option where sharing the device and its caches costs no
lock (one executor mutates them), surface creation stays where the platforms require it (the
owner), and the placement is a constructor argument rather than a second protocol, so a platform
moves to a raster thread without changing a command or an answer.

§1. **One `GpuContext` per application, owned by the raster executor.**

```rust
// flui-engine
pub struct GpuContext {
    instance: wgpu::Instance,           // the one the owner's SurfaceFactory clones (§2)
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    domain: Arc<DeviceDomain>,          // ADR-0100; shared only with wgpu's device callbacks
    pipelines: PipelineLibrary,         // keyed by (PipelineKey, wgpu::TextureFormat)
    glyphs: TextAtlas,                  // ADR-0067, one per context
    images: TextureCache,               // keyed by TextureKey, which pins the allocation
    slots: SlotTable<SurfaceSlot>,      // §3
    epoch: ContextEpoch,                // §7
    _one_mutator: PhantomData<Cell<()>>, // Send, !Sync
}
```

- There is one per application. The raster executor owns it (§10); nothing else holds it or a
  reference into it, it is never a `static`, and it is never reached ambiently (ADR-0097, ADR-0091
  §3). It is `Send`, so a threaded executor can take it, and `!Sync`, so no `&GpuContext` crosses
  a thread; the only locks inside are the device domain's, which wgpu's callbacks share
  (`crates/flui-engine/src/device_domain.rs:390-402`).
- **What is keyed by content is shared; what is keyed by the presentation is the slot's.** The
  context holds pipelines, rasterised glyphs, decoded images and tessellated paths. Their keys
  hold everything the value depends on: a `GlyphKey` names the face by a font blob id, the glyph,
  the exact size bits, the variation, the subpixel bin, hinting and synthesis
  (`crates/flui-painting/src/glyphs/key.rs:154-162`), blob ids come from a process-wide counter
  that never repeats (`linebender_resource_handle`, its src/blob.rs line 90), and the atlas's registry
  holds each blob while an entry names it (`crates/flui-engine/src/glyph_atlas.rs:7-11`); a
  `TextureKey` holds the image handle, so its address cannot be reused while cached
  (`crates/flui-engine/src/texture_cache.rs:64-94`); and the pipeline key gains the target
  format, because two windows can be configured with different formats while `PipelineCache`
  holds one today (`crates/flui-engine/src/pipeline_cache.rs:429-451`). The slot holds the
  surface and its configuration, its frame protocol and damage history
  (`crates/flui-engine/src/frame_protocol.rs:1-9`), its committed images (ADR-0100), its viewport,
  its `SurfaceGeneration` counter (§6) and its external textures, whose `TextureId`s each
  presentation's registry mints on its own (`crates/flui-layer/src/texture_registry.rs:103-113`).
- `wgpu::PipelineCache` reaches pipeline creation only where the adapter has
  `Features::PIPELINE_CACHE`, which wgpu implements on Vulkan alone
  (`wgpu-types/src/features.rs:1163-1172`); every other site passes `None`, as all fourteen
  pipeline sites do today. Its data lives for the context's life and is not persisted: where it
  would be stored and how a driver update invalidates it is a separate decision.
- **An atlas entry ages by queue submission.** The glyph atlas and the image cache stamp an entry
  with the submission that last used it; the counter advances once per `queue.submit`, and an
  entry used at or after the start of the oldest shown slot's latest frame is immune from
  eviction. Work recorded and not yet submitted therefore never loses an entry, every shown
  window keeps its working set, and an upload into an evicted region reaches the GPU after every
  submission that read it, in queue order. The growth rule of ADR-0067 is unchanged.

§2. **The owner makes surfaces through a factory over the context's instance, and the instance's
display is the platform's connection.**

```rust
// flui-platform
pub trait DisplayConnection: HasDisplayHandle + Send + Sync + 'static {}
// OwnerPlatform::display_connection(&self) -> Arc<dyn DisplayConnection>, one per host life

// flui-engine
#[derive(Clone)]
pub struct SurfaceFactory {
    instance: wgpu::Instance,
    _owner: PhantomData<Rc<()>>,        // !Send + !Sync: surfaces are made on the owner thread
}
impl SurfaceFactory {
    pub fn create(&self, target: Arc<dyn SurfaceTarget>) -> Result<UnconfiguredSurface, EngineError>;
}
pub struct UnconfiguredSurface {                // Send
    lease: SurfaceLease<wgpu::Surface<'static>>, // ADR-0063: the surface and the target it needs
    target: Arc<dyn SurfaceTarget>,              // the same target, which the slot notifies (§5)
}

impl GpuContext {
    pub async fn bootstrap(
        display: Arc<dyn DisplayConnection>,
        first: Option<Arc<dyn SurfaceTarget>>, // None: offscreen slots only (headless, tests)
    ) -> Result<GpuBootstrap, EngineError>;    // EngineError::NoSupportedGpu when no backend answers
}
pub struct GpuBootstrap {
    pub context: GpuContext,
    pub factory: SurfaceFactory,
    pub first: Option<UnconfiguredSurface>,
}
```

- **The display is the host's, not a window's.** wgpu reads the instance's display at every
  `create_surface`, with `expect`, and refuses a surface on another display
  (`wgpu-core/src/instance.rs:205-215`; `wgpu-types/src/instance.rs:50-61`). Each instance is
  built from its window's target today; shared, an instance built that way would panic on the
  next window's surface once its first window had closed, because a retired target answers
  `Unavailable` from then on (`crates/flui-engine/src/window_target.rs:63-69`). Each host hands
  out one `DisplayConnection` for its life: the X11 and Wayland connections, winit's
  `OwnedDisplayHandle`, and on hosts without a connection a constant handle. A host's connection
  answers the same `RawDisplayHandle` as its targets.
- **The factory is owner-local.** `SurfaceFactory` is `!Send` and `!Sync`, so code that makes a
  surface on the raster thread does not compile; that is where the Apple and Android targets
  answer `Unavailable` (`surface_target.rs:18-25`). It probes the target before creating, as
  ADR-0063 decision 3 types it, and the surface reaches the executor in an `Attach` or `Restore`
  command (§3). `wgpu::Instance` is `Clone + Send + Sync` (`wgpu/src/api/instance.rs:31-36`), so
  the factory and the context share one instance. The executor makes no surface either: a
  window surface still lost after a reconfigure is dropped, the frame answers
  `EngineError::SurfaceReplacementNeeded`, and the slot's record says it wants a surface
  (`SlotRecord::wants_surface`) until the owner sends one in a `Restore`. A `Restore` the backend
  cannot use is named by `SlotRecord::surface_refused`, so the owner tells it from a surface
  dropped after one it took, and stops retrying that frame; one over the device's size limit
  keeps its surface, as after a rebuild, and waits for a size that fits.
- **The bootstrap runs ADR-0244 decision 1's probe once**, over the first window when there is
  one: each backend attempt in its own instance, the winning instance kept by both the factory
  and the context. It is `async`; native runners block on it once at startup, and the web runner
  awaits it before its first `Attach`.
- **The first surface picks the adapter for the context's life.** A later surface the adapter
  cannot present to (`wgpu::Adapter::is_surface_supported`) is refused at `Attach` with
  `EngineError::IncompatibleSurface`, reported where the window was opened; a WebGL2 context
  presents to one canvas, so a second canvas on the GL backend is refused the same way, and the
  change that implements it records the one-canvas limit of WebGL2 in `docs/gaps.md`. WebGPU has
  no such limit.

§3. **A presentation is a slot, fed by closed commands.**

```rust
// flui-engine; every enum here is exhaustive (ADR-0068 §2)
pub enum RasterCommand {
    Attach { slot: SlotId, address: PresentationAddress, target: SlotTarget, config: SlotConfig },
    Release { slot: SlotId },                          // the window's surface goes away
    Restore { slot: SlotId, surface: UnconfiguredSurface },
    Detach { slot: SlotId },
    RetryDevice,                                       // the application asked again (ADR-0267)
    Frame { slot: SlotId, snapshot: SceneSnapshot },
    Configure { slot: SlotId, change: SlotChange },
    Trim(MemoryPressureLevel),
}
pub enum SlotTarget { Window(UnconfiguredSurface), Offscreen { size: DeviceSize, format: wgpu::TextureFormat } }
pub enum SlotChange { Size(DeviceSize), Shown(SlotVisibility), Color(DisplayColor), Latency(FrameLatency) }
pub enum RasterSendError { Full(RasterCommand), StaleSlot(RasterCommand), Gone(RasterCommand) }
```

- **Identity.** `SlotId` is generational (`new_gen(index, generation)`), minted on the owner side,
  so attaching never waits for the raster side. An index is freed only after the slot's record
  reports it detached, and an index whose generation would overflow is retired, never reused. A
  command naming a stale `SlotId` is refused with `StaleSlot`. A slot is bound to the
  `PresentationAddress` it was attached with, and a frame stamped with another address is refused
  as today (`RasterSubmitError::AddressMismatch`).
- **Carriers.** `Attach`, `Release`, `Restore`, `Detach` and `RetryDevice` travel in order on a
  bounded lifecycle queue (`LIFECYCLE_CAPACITY`, 64); a full queue answers `Full` with the command,
  and the owner sends it again after the executor's next wake. `Frame` and `Configure` coalesce
  in the slot's inbox: a later frame supersedes a pending one for presentation, the scene state
  the superseded frame carried folds into it as ADR-0304 decides, and each kind of change keeps
  its latest value. `Trim` coalesces per context to the strongest level, applied before the next
  frame. No send drops a command: every answer returns it or accepts it, and `Gone` answers once
  the executor has stopped (§9).
- **Order.** A turn drains the lifecycle queue before any inbox, so a frame never reaches a slot
  that is not yet attached, and a frame stamped before a `Restore` is refused on its surface
  generation (§6).
- **Locks.** The inbox's lock is held only to swap values; the raster side never holds it across
  acquire, render or present, and no drop and no wake runs under it: a superseded scene is dropped
  after the lock is released, since its destructor can be user code.
- `with_backend` (`raster_owner.rs:1258`, `raster_lane.rs:283-284`) is deleted; §12 maps each of
  its production callers to a command.
- A `SlotTarget::Offscreen` slot renders to a texture the executor reads back, so the headless
  renderer becomes a `GpuContext` with offscreen slots, on the path windows use
  (`crates/flui-engine/src/headless.rs:15`, `:104-109`), and the confirmation below runs in CI
  without a window.

§4. **A queued frame settles by its stamp.**

```rust
// flui-runtime
pub enum SubmitVerdict { Presented, NoPresent, NotShown, Retry, SurfaceStale, DeviceLost, Failed, Queued }
pub struct FrameSettlement { pub address: PresentationAddress, pub epoch: FrameEpoch, pub verdict: SubmitVerdict }
pub enum Settled { Applied, Stale }
impl UiRuntime { pub fn settle_frame(&mut self, settlement: FrameSettlement) -> Settled; }
```

- Inline (§10), `FrameSink::submit` runs the executor's turn and answers the frame's own verdict,
  as today. Threaded, it answers `Queued` once the inbox has the frame; the UI runtime keeps the
  frame's post-submit work (the tree revision it commits, its input regions, its staged layer
  reads, its frame-aligned haptics, its presented timestamp) in a record keyed by the frame's
  address and epoch, captured at submit, so a settlement commits the frame it names and not the
  tree as it is when the answer arrives. Input regions publish when their frame settles
  `Presented`, so hit testing matches the pixels on screen; a frame that settles anything else
  leaves the regions of the last presented frame in force.
- The executor writes each settlement into the slot's record, never `Queued`; the record holds
  every undrained settlement in a ring of the placement's depth, and the executor starts no frame
  of a slot whose ring is full, so none is overwritten. A frame superseded in the inbox gets no
  settlement of its own: the settlement of the frame that superseded it settles it too, and the
  UI runtime folds its record into that frame's. Then the executor calls the host's wake, with no lock held
  and contained (`raster_owner.rs:205-227`); the owner drains the record on its own thread and
  calls `settle_frame` after its own borrows are released (ADR-0287 §1).
- A settlement whose address or epoch no pending record holds is `Stale`, traced and ignored:
  `FrameEpoch` counts within one runtime's life, and the address carries the runtime's
  generational id.
- Every pending frame settles on every exit: `NotShown` while the slot has no surface or is
  hidden (§5), `DeviceLost` while the device is lost (§8), `Retry` after a contained panic (§7),
  `Failed` for each frame still pending when the executor stops or dies (§9). A detached slot's
  pending records leave with its presentation.

§5. **Pacing, restated per slot.** ADR-0045 decision 3 and the rule of decision 5 stand, per slot:

- `FrameClock`'s in-flight gate paces production, and the slot's record carries the count. The
  depth is the placement's (§10): 1 inline, 2 threaded. wgpu's `desired_maximum_frame_latency`
  stays the slot's `FrameLatency` (ADR-0257 decision 4), set at `Attach` and changed by
  `Configure`, and stays decoupled from the depth; the GL backend ignores it as before.
- The slot calls its `SurfaceTarget::pre_present_notify` directly, immediately before
  `queue.present` and only for a frame that will present; `flui-engine` already reaches
  `flui-platform` (`crates/flui-engine/Cargo.toml:50-52`), so no hook closure is installed.
- `FallbackWake`'s deadline applies only when the slot's in-flight count is zero and its last
  settlement did not present. While a frame is in flight the owner keeps a `WaitUntil` floor, so a
  lost wake costs at most one floor interval.
- The raster side never asks a window for a frame; a frame is asked through the window's
  `RedrawRequester` on the owner's side (ADR-0254 decision 5).
- **A hidden slot is not acquired.** `Configure { change: Shown(..) }` carries the window's
  visible, minimised and occluded state (`crates/flui-platform/src/window_state.rs:150-166`). A
  frame for a slot that is not shown applies its scene state and settles `NotShown`; the slot
  redraws in full when it is shown again. On one raster thread an acquire on a hidden window can
  wait out wgpu's second for every other window, and Metal ignores that timeout
  (`wgpu-hal/src/metal/surface.rs:349`), skipping an occluded window only by asking its
  `NSWindow` (`:354-370`).

§6. **One `SurfaceGeneration` counter per slot, and no resource axis.** ADR-0045 decision 4's rule
holds per slot: the counter lives in the slot's inbox state and changes only under its lock;
`Configure { change: Size(..) }` mints there and returns the generation the owner stamps with,
the raster side mints there on a lost surface, and a frame renders only when its stamp equals the
applied configuration's generation. `SurfaceGeneration::ZERO` still means no surface, and a size
with a zero or negative side mints no generation and leaves the slot unconfigured, as `resize`
answers `None` for it today (`raster_owner.rs:928`). A minted size the backend refuses for now
(an error whose `Recoverability` is `Recoverable`) or whose resize panics stays pending, and the
next turn retries it; a size it always refuses is not retried: the slot keeps its previous
configuration, so frames stamped with that generation settle `SurfaceStale`, the failure waits in
the slot's record, and the executor warns once for that size until the owner sends another one.
A size pending or refused is a barrier for every newer generation: neither a lost surface nor a
`Restore` authorises frames laid out for a size the slot does not have.
`GpuResourceGeneration`, `FrameStamp::gpu_resource_generation`,
`RasterHandle::bind_gpu_resource_generation` (`raster_owner.rs:954`) and
`PumpOutcome::ResourceOutdated` are deleted: a frame carries CPU data only, every GPU object it
needs is derived inside the executor, and the executor re-derives all of them after a rebuild
(§8), so no frame can be stale against a device. ADR-0037 §8 holds with the slot as the
presentation's raster owner.

§7. **A panic is contained per command, and costs the shared caches, not the pipelines.** The
executor runs each command under its own catch, in both placements.

- The panicking frame settles `Retry`; its payload goes into the slot's record, the first one kept
  and later ones discarded under containment (ADR-0281). The owner reports it at the slot's address
  through `UiRuntime::report_contained_panic` (ADR-0258 §1), and the next attempt waits out
  ADR-0258 §6's backoff, so a panic on every frame costs one attempt per backoff interval.
- The executor then rebuilds what a panic can tear and other slots share: the glyph atlas, the
  image and path caches and the scratch pools; `ContextEpoch` advances. A slot's recording derived
  from a shared cache carries the epoch it was derived under and is recorded again when the epoch
  moved. The panicking slot redraws in full; a slot whose retained scene the panic tore is rebuilt
  from the owner's next scene as ADR-0304 decides. The other slots keep their committed images,
  which only a completed commit changes (ADR-0100).
- The pipeline library is kept: an entry is inserted after its pipeline is built
  (`pipeline_cache.rs:551-552`), so a panic leaves a complete entry or none.
- A panic inside a submission quarantines the device domain, which is a device loss (ADR-0100;
  §8).
- User code the executor runs: the host's wake, `pre_present_notify`, a `tracing` subscriber, and
  the destructor of a dropped scene. None runs under the inbox lock. A `tracing` event inside a
  command can run while that command's slot is half-updated, which is why a panic there is
  contained and costs the caches above.

§8. **A lost device is the context's, and the rebuild keeps the surfaces.**

- wgpu's device-lost callback is installed once per device and marks the domain lost
  (`renderer.rs:1641`). Every slot's frames settle `DeviceLost` until the rebuild succeeds.
- One `DeviceRebuilds` per executor, which owns the one context, holds ADR-0267's backoff (16 ms
  doubling to a one-second ceiling) and its ceiling (ten failures at it) on the clock the executor
  is built with, so a manual clock walks it; a failed attempt's wait runs from when the attempt
  ended, since the request blocks for as long as the driver takes. The executor's record
  publishes the state as
  `DeviceHealth { Healthy, Rebuilding { attempts }, GivenUp { attempts } }`, which the host maps to
  ADR-0267's `DeviceRecovery { Pending, Failed, Abandoned }`, with when the next rebuild may run
  (`RasterSender::device_retry_at`, for the host's wake) and why the last one failed
  (`RasterSender::take_device_failure`); that record is what `is_device_lost` reads instead of the
  backend (`raster_lane.rs:289`). A loss that this turn's own frames meet is published after the
  slots are served, with its first rebuild due now (`device_retry_at` answers the current instant),
  so the owner runs the next turn. A rebuild that panics counts as a failure and its panic is
  resumed once the turn has served every slot; the context counts its device lost until every
  slot is on the new one, so a rebuild that unwinds part way is asked for again from the start
  (what an unwound attempt refused is kept and answered by the attempt that finishes),
  and the executor restamps every slot whenever a lost device renders again, whichever way it
  came back. A backend that cannot rebuild where the executor runs (the web, below) is given up
  at the first due attempt. After `GivenUp` the context stays given up until
  the application sends `RetryDevice`; nothing retries on its own.
  That retry runs ADR-0244 decision 1's probe again in fresh instances, because the backend whose
  rebuilds exhausted the backoff may be the cause: the factory is replaced and every slot gets a
  new surface from the owner in a `Restore`.
- A rebuild that succeeds mints each configured slot's next surface generation, so a frame
  stamped before it is refused as after a `Restore`; a size pending or refused keeps its barrier
  (§6), and every slot redraws in full.
- The rebuild runs where the executor runs (§10). It keeps the instance and every surface,
  requests an adapter and device on that instance, compatible with the surface of the first window
  slot that still holds one (`RequestAdapterOptions::compatible_surface`), rebuilds the
  pipelines and caches, and reconfigures each slot's surface from the new adapter's capabilities
  (ADR-0063 decision 6's rule for a rebuilt surface). On a hybrid-GPU machine the new adapter can
  differ from the old one; a slot it cannot present to settles `SurfaceStale` and reports
  `EngineError::IncompatibleSurface` at that slot's address, an error of that slot and not of
  the context. A slot whose reconfigure fails settles `SurfaceStale`, and its
  record asks the owner for a new surface, which the owner makes with the factory and sends in a
  `Restore`. A slot over the new device's size limit keeps its surface unconfigured and holds
  `InvalidTargetSize` until a resize fits, as an offscreen slot does. Each slot redraws in full
  and re-uploads its external textures from the newest image of each that it keeps
  (ADR-0233).
- On the web the rebuild is a future the owner spawns; frames settle `DeviceLost` until it ends.

§9. **Teardown is bounded and the executor's health is typed.** ADR-0045 decision 7 stands per
executor and slot.

- `ExecutorHealth { Running, ShutDown, Died }` rides the executor's record; the two non-running
  states log at `error`. Once it is not `Running`, every send answers `Gone` with its command, and
  the owner settles every frame still pending `Failed`, since the executor no longer can.
- `Detach` and `Release` complete before the native window is destroyed or its surface given
  back. Inline they complete within the call. Threaded, the owner waits for the slot's record with
  a deadline (`DETACH_DEADLINE`, two seconds, above wgpu's one-second acquire timeout), with no
  borrow of the host or a runtime held. On Win32 the wait services sent messages only
  (`MsgWaitForMultipleObjectsEx` with `QS_SENDMESSAGE`): a present can send a message to the
  window's thread, and a message thread that waits on the render thread without serving it
  deadlocks (Microsoft's DXGI guidance, below).
- On a timeout the window is quarantined, not closed: the presentation leaves, its window and
  target are kept until the process exits, and the executor is marked `Died`. A raster thread
  wedged past the deadline holds the context, so every window stops drawing, not one; the
  application learns it from `ExecutorHealth` and from each frame's `Failed` settlement.
- `Died` is terminal for the application's rendering, as a dead runtime is under ADR-0258 §2: the
  owner does not bootstrap a second context while the wedged thread still holds the first and its
  quarantined windows, and the application decides whether to exit.

§10. **One executor, two placements, per platform.**

```rust
pub enum RasterPlacement { Inline, Threaded }   // depth 1 and 2 (§5)
pub struct RasterExecutor<B: RasterBackend = GpuContext> { /* the backend, the queue, the inboxes */ }
impl<B: RasterBackend> RasterExecutor<B> {
    pub fn turn(&mut self) -> TurnOutcome;                        // inline: one pass
    pub fn render_lent(&mut self, slot: SlotId, scene: &Scene, reset_fonts: bool)
        -> Result<PresentDisposition, EngineError>;
}
#[cfg(not(target_arch = "wasm32"))]
impl<B: RasterBackend + Send + 'static> RasterExecutor<B> {
    pub fn spawn(self) -> std::io::Result<ExecutorHandle>;        // threaded: a named raster thread
}
```

- Inline, the owner holds the executor and runs its turn from `FrameSink::submit` and after each
  lifecycle command. Threaded, `spawn` moves the executor and its context onto one raster thread,
  and the owner holds only an `ExecutorHandle`; one raster thread serves every slot (ADR-0091 §4).
- The target placement is ADR-0091 §5's table: threaded on Win32 and Linux; inline on macOS
  (wgpu-hal's Metal acquire messages `NSWindow` from the acquiring thread,
  `wgpu-hal/src/metal/surface.rs:354-370`) and on the web; inline on Android and iOS until a
  device run meets the budget; inline in headless and testing. A platform becomes threaded when a
  run on it meets ADR-0091 §5's pacing budget, including a two-window case with one window
  occluded; `docs/platform-parity.md` records the run. The occluded case is part of the budget
  on every platform, inline ones included, because one executor serves every window in both
  placements.
- `render_lent` serves the hot-reload plugin's scene, lent by reference for the length of one
  call (`crates/flui-app/src/app/hot_reload.rs:365-395`, `crates/flui-app/src/app/runner/android.rs:608`).
  It exists only on the executor the owner holds, which a threaded placement does not have, so a
  lent scene cannot cross a thread.

§11. **wgpu is the only rasteriser.** There is no CPU raster backend; the single-rasteriser stance
of `crates/flui-engine/ARCHITECTURE.md:8-13` stands. A machine without a usable GPU gets
`EngineError::NoSupportedGpu` from the probe (ADR-0244 decision 1). CI rasterises on Mesa's
software Vulkan and GL. Colour and coverage correctness keep their narrow CPU oracles
(`crates/flui-engine/src/blend_oracle.rs:1-16`, `crates/flui-engine/src/aa_oracle_tests.rs:1-10`,
`Color::blend` at `crates/flui-painting/src/styling/color.rs:912`), and runtime tests that need no
GPU drive a `FrameSink` (`crates/flui-runtime/src/sink.rs:96`;
`crates/flui-runtime/src/testing.rs:356`). `RasterBackend` stays in `flui-engine` as the
executor's GPU-free seam, as ARCHITECTURE.md describes it: `GpuContext` and the testing stand-in
implement it, its methods name the `SlotId` they act on, and it loses its `Send` supertrait
(`crates/flui-engine/src/raster.rs:103`); only `spawn` asks for `Send`.

§12. **The lane holds no lock, and nothing reaches past the commands.** `Arc<Mutex<RasterLane>>`
(`desktop.rs:245-252`) and `LaneStamp`'s mutex (`raster_lane.rs:72-81`) are removed with their
reason, which no longer holds: the callbacks they serve take `'static` closures with no `Send`
bound (`window_routes.rs:185-237`, `window_install.rs:269`). The owner side is an owner-local
`RasterHost` holding the factory, the placement and each slot's handle, in an `Rc<RefCell<_>>`; a
re-entrant frame dispatch that finds it borrowed answers `SubmitVerdict::Retry`, as the
`try_lock` does today (`raster_lane.rs:486-490`). On the web, `Arc<Mutex<Option<Renderer>>>`
(`crates/flui-app/src/app/runner/web.rs:79`) and `DirectSink` (`raster_lane.rs:513`, `web.rs:325`)
are removed for the inline executor. No `unsafe impl Send` is added and no `Send` position joins
the facade. Every production use of `with_backend` becomes:

| Call site | What it does | Becomes |
|---|---|---|
| `crates/flui-app/src/app/runner/surface_lifecycle.rs:144` | follows the display's colour | `Configure { change: Color(..) }` |
| `surface_lifecycle.rs:576` | releases or rebuilds the surface on Android and iOS | `Release`, then `Restore` with a surface the factory made |
| `surface_lifecycle.rs:657` | retries a failed surface recreation | `Restore` once the owner's retry makes a surface |
| `android.rs:608` | renders the hot-reload plugin's lent scene | `RasterExecutor::render_lent`, inline only |
| `device_recovery.rs:403` | rebuilds a lost device before a frame | the context's rebuild (§8); `RetryDevice` after `Abandoned` |
| `device_recovery.rs:438` | the same rebuild after a frame | as above |
| `raster_lane.rs:289` | asks whether the device is lost | reads `DeviceHealth` in the executor's record (`RasterSender::device_health`) |
| `raster_lane.rs:354` | applies external texture updates | the updates ride the slot's `Frame` |

### Consequences

- Good, because one device, one pipeline per key and format and one glyph atlas serve every
  window, and a second window costs a surface and its slot.
- Good, because surface creation on the wrong thread, a lock around the lane, a lent scene on the
  raster thread and a command dropped on a full queue are each a compile error or a typed answer.
- Good, because a window opened after the first one closed gets a surface instead of a panic in
  wgpu's display check.
- Good, because once a platform's placement is threaded its owner thread stops waiting on the
  GPU: on vsync where the present blocks, on device rebuilds and on pipeline compiles.
- Bad, because a threaded frame's post-submit work moves to its settlement, a second path in the
  UI runtime beside the inline one; the inline path is unchanged.
- Bad, because a panic in one slot rebuilds the shared glyph atlas and image caches for every
  window, and a device loss redraws every window in full; per-window stacks paid neither.
- Bad, because one raster thread presents the windows in turn, so a `Fifo` present on one window
  can delay another's; the per-platform run measures it before a platform is threaded.
- Bad, because a raster thread wedged past the teardown deadline stops every window, where a
  wedged per-window lane stopped one.
- Neutral, because `WindowPolicy::Shared` stays refused: slots remove its raster reason
  (`crates/flui-app/src/app/runner/secondary_window.rs:924-930`), and ADR-0037 §11's element forest
  is still missing.
- Neutral, because the web build's `fragile-send-sync-non-atomic-wasm` feature is justified by
  "flui-app's Send-bounded frame callbacks" (`crates/flui-engine/Cargo.toml:115-118`), which §12
  removes; the change that moves the web runner onto the executor drops the feature once no `Send`
  bound is left on the web path.

### Confirmation

- Compiler: `SurfaceFactory` is `!Send` (compile-fail fixture `surface_factory_is_owner_local`, to
  be added); `GpuContext` is `Send` and not `Sync` (`static_assertions` beside the type, to be
  added, as `Renderer` has them at `renderer.rs:914-915`); every enum of §3 and §4 is exhaustive,
  so a new command or verdict breaks every match. The fixture `raster_backend_requires_send` is
  deleted with the supertrait, and `renderer_new_rejects_borrowed_window` becomes
  `surface_factory_rejects_borrowed_window`.
- Gates: `cargo xtask globals` admits no new `static` or `thread_local!`, so none holds a
  context, a device or an atlas; the `cargo xtask send-impls` and `cargo xtask send-positions`
  lists do not grow.
- Tests to be added, in `flui-engine` on offscreen slots under `cargo xtask gpu-test` (lavapipe and
  llvmpipe):
  - `every_slot_shares_one_device_one_glyph_atlas_and_one_pipeline_per_format`: several slots,
    counting instances, devices, atlases and pipeline builds;
  - `a_panic_in_one_slot_rebuilds_the_shared_caches_and_keeps_the_pipelines`: the panicking frame
    settles `Retry`, another slot's committed image is unchanged, no pipeline is built again;
  - `a_device_rebuild_keeps_the_surfaces_and_redraws_every_slot_in_full`;
  - `a_hidden_slot_is_not_acquired_and_settles_not_shown`.
- Tests to be added through the public API, without a GPU, in `flui-engine`'s `tests/` against
  the testing backend: `a_full_lifecycle_queue_returns_the_command`,
  `a_stale_slot_id_is_refused_after_detach`,
  `a_superseded_scene_is_dropped_after_the_inbox_lock_is_released` (its destructor sends to the
  same slot), `a_full_settlement_ring_holds_the_next_frame`,
  `a_stopped_executor_settles_every_pending_frame_failed`,
  `a_size_the_backend_always_refuses_is_not_retried_and_warns_once`,
  `a_window_slot_attaches_unconfigured_and_its_first_size_configures_it`,
  `a_frame_stamped_before_a_restore_is_refused_on_its_generation` (window surfaces from wgpu's
  no-op backend); compile-fail `surface_factory_is_owner_local` and
  `surface_factory_rejects_borrowed_window`; and in `flui-runtime`'s
  `tests/main.rs`: `a_queued_frame_settles_by_its_stamp_and_a_stale_settlement_is_ignored`.
- Conformance, to be added in `flui-native`: `a_surface_targets_display_equals_the_platforms_connection`,
  one row per host, in `flui-native`'s Linux suites: the Wayland row under a headless `weston`,
  the X11 row under `Xvfb`, the winit and headless rows without a display server.
- Kept and rewritten per slot: the raster owner's protocol tests
  `latest_frame_wins_supersedes_pending`, `stale_surface_generation_is_rejected_before_render`,
  `trims_coalesce_to_the_strongest_level_and_apply_before_the_frame`,
  `superseded_damage_folds_into_the_survivor`, `device_lost_completion_retires_the_ticket`,
  `quarantined_domain_requests_device_recovery`,
  `panic_mid_render_retires_the_ticket_and_capacity_is_usable_again`,
  `every_failure_boundary_retires_its_ticket`,
  `stalled_capacity_then_released_wakes_with_no_external_trigger`,
  `transient_render_failure_is_durable_without_a_telemetry_ack` and
  `shutdown_and_backpressure_are_typed_and_live`; ADR-0244's `gpu_backend_fallback_order`; and
  ADR-0267's `a_frame_dropped_on_a_lost_device_waits_out_the_backoff`,
  `a_failed_rebuild_is_reported_with_the_dropped_frame`,
  `a_given_up_rebuild_holds_frames_until_the_application_asks_again`,
  `a_rebuild_failing_at_the_ceiling_is_given_up_until_the_application_asks`,
  `a_lost_device_waits_out_its_rebuild_backoff` and
  `a_given_up_device_renders_again_when_the_application_asks`.
- Runs: a platform's placement becomes threaded only with its row in `docs/platform-parity.md`
  (§10).

## Pros and cons of the options

### 1. A GPU stack per window

- Good, because it is built, and a failure in one window touches nothing else.
- Bad, because every window pays a device, a pipeline set and an atlas (ADR-0091 §3).
- Bad, because the instance holds a window's display, so a later window's surface depends on an
  earlier window staying open.
- Bad, because it gives no answer for where a GPU wait runs other than the owner thread.

### 2. One context, one executor, slots fed by commands (chosen)

- Good, because sharing needs no lock: one executor mutates the context.
- Good, because the placement is a constructor argument, so inline and threaded share every
  command, answer and test.
- Bad, because a panic or a device loss in one window costs every window's shared caches or a full
  redraw.
- Bad, because a threaded frame settles later than its submit, which the UI runtime has to carry.

### 3. A raster thread per window over one shared context

- Good, because a blocked present delays only its own window.
- Bad, because the atlas and caches need a lock taken by every frame of every window, which the
  frame path forbids (ADR-0091 §4 weighed the same).
- Bad, because a device rebuild must stop several threads together.

### 4. Inline everywhere, a thread only once a measurement asks

- Good, because it adds no thread until one is shown to help.
- Bad, because part of the measurement is already in: on the Vulkan and Wayland path every
  presented `Fifo` frame blocks its thread at display cadence, and on every platform a device
  rebuild blocks it (`renderer.rs:1751-1760`, `device_recovery.rs:33`); on the one owner thread
  that wait is every runtime's input latency.
- Neutral, because it is the order the code moves in (Migration); what it lacks is the target and
  the criterion, which ADR-0091 §5 already gives per platform.

### 5. A contract in `flui-layer` with a CPU backend

- Good, because a machine without a GPU would still draw.
- Bad, because the probe already answers that machine with `NoSupportedGpu`, and CI already
  rasterises without hardware on Mesa's software drivers.
- Bad, because a second rasteriser doubles every pixel rule and its conformance suite, for no
  user the code has; the narrow CPU oracles already check the rules that need one.
- Bad, because moving the contract to `flui-layer` served only the optional move ADR-0091 §6 made
  conditional on it, which this decision withdraws.

## Migration

The code moves in this order, each a change of its own and the inline placement first, because it is
the shipping one on every platform: the context and the executor inline, with the headless
renderer on offscreen slots; the desktop, Android and iOS runners on slots, with `with_backend`,
the lane's lock and the hook closure removed; the web on the inline executor, with `DirectSink`
removed; the settlement path in the UI runtime; then the threaded placement on Linux and Win32, each
when its run meets the budget.

## More information

- ADR-0045, decision by decision:

  | ADR-0045 | Here |
  |---|---|
  | 1. The split rule; surface creation owner-affine; macOS inline; the layout size on the owner | restated (§2, §10; `FrameSink::surface_size` reads the owner's cell) |
  | 1. Exactly one value crosses down; `RasterBackend: Send`; the renderer moves into the thread | replaced: commands cross down and the slot's record crosses up (§3, §4); `Send` is asked by `spawn` alone (§11) |
  | 2. `GpuServices` per owner thread, `ReplaceServices`, owner-built replacement surfaces, recovery as a whole-thread event | replaced by one context per application (ADR-0091 §3) and a rebuild that keeps the surfaces (§8) |
  | 2. The first window picks the adapter, for good | restated (§2) |
  | 3. The in-flight gate, the pre-present notification, `FallbackWake` only when nothing is in flight and the last frame did not present, the occluded window | restated per slot (§5); the exit criterion was replaced by ADR-0091 §5 |
  | 4. One `SurfaceGeneration` counter, `ZERO` as no surface | restated per slot (§6) |
  | 4. The `GpuResourceGeneration` axis | withdrawn (§6) |
  | 5. The raster side never asks for a frame; the floor while in flight | restated (§5); the relay was replaced by ADR-0254 decision 5 |
  | 6. `PipelineDepth`, `RasterMode` | replaced by `RasterPlacement` and its fixed depth (§5, §10) |
  | 7. Health, the bounded teardown, quarantine | restated per executor and slot (§9) |

- `gfx-rs/wgpu@40f4a34eba`: `wgpu-types/src/instance.rs:50-61` (the instance's display, required
  for GL on Wayland, ignored by Vulkan, Metal and D3D12); `wgpu-core/src/instance.rs:56-62`,
  `:205-215` (kept for keepalive and checked at every surface); `wgpu-core/src/present.rs:32`,
  `:178` (the fixed acquire timeout); `wgpu-hal/src/metal/surface.rs:349`, `:354-370` (Metal
  ignores the timeout and messages `NSWindow`); `wgpu-hal/src/gles/egl.rs:769-871` (the EGL
  display is chosen when the instance is made); `wgpu-types/src/features.rs:1163-1172`
  (`PIPELINE_CACHE` on Vulkan only).
- Microsoft, "DXGI: multithread considerations":
  <https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/d3d10-graphics-programming-guide-dxgi#multithread-considerations>
  (a present can send messages to the window's thread; the message-pump thread never waits on the
  render thread).
- Revisit the placement of a platform when its run shows the threaded placement missing the budget
  where the inline one meets it; revisit the shared caches if a measured workload shows one
  window's eviction costing another's frame time.
- `LIFECYCLE_CAPACITY` (64) and `DETACH_DEADLINE` (two seconds) are argued, not measured; each is
  a constant, not a contract, and moves when a run shows another value wins.
