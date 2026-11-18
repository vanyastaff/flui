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
  - [ADR-0027](ADR-0027-owner-affine-ui-realms.md) §4's former shutdown-completion and
    frame-telemetry rows, §5's raster protocol and §7 steps 3 and 4 only (acknowledgements on a
    lossy telemetry lane, a one-shot shutdown completion, a raster owner per presentation): a slot
    of the one executor settles each frame with a verdict.
- **Superseded-by:** [ADR-0371](ADR-0371-before-the-first-release-no-xtask-gate-holds-a-rule-about-the-sources.md)
  for the `xtask globals` and `xtask send-impls` gates only (Confirmation): the rule stands and its
  reader confirms it.
  [ADR-0375](ADR-0375-before-the-first-release-a-pull-request-runs-a-fixed-gate-and-no-api-contract-gate.md)
  for the `xtask send-positions` gate only (Confirmation): the rule stands and its reader confirms
  it.
- **Amends:** [ADR-0091](ADR-0091-one-owner-thread-isolated-realms-raster-thread.md) §3, §4 and §7
  (their shape: the executor owns the context, and a slot per presentation holds what §4 calls
  its raster owner state and §7's external textures);
  [ADR-0254](ADR-0254-window-contract.md) decision 5 (the target goes to the owner's
  `SurfaceFactory`, and the instance's display is the platform's connection);
  [ADR-0267](ADR-0267-a-lost-gpu-device-is-rebuilt-on-a-backoff-and-given-up.md) (the rebuild is
  the context's, once per context); [ADR-0067](ADR-0067-engine-owned-glyph-atlas.md) (one atlas
  per context, aged by queue submission); [ADR-0347](ADR-0347-a-frame-request-carries-the-hosts-frame-times-and-reports-that-name-their-frame.md)
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
| The lane sits behind `Arc<Mutex<_>>`, justified by callbacks that "require `Send` closures"; the registrations take none, and the frame driver beside it already holds a `std::rc::Weak` | `desktop.rs:245-252`, `:419-425`; the former `raster_lane.rs:72-81`; `crates/flui-app/src/app/runner/window_routes.rs:185-237`; `crates/flui-app/src/app/runner/window_install.rs:269` |
| Eight production calls reach past the raster protocol into the backend | the former `raster_owner.rs:1258` and the table in §12 |
| The GPU-resource axis of a frame stamp is `ZERO` on both sides, and its rejection is unreachable | `raster_lane.rs:344-348`, `:398-407` |
| A presented `Fifo` frame on the Vulkan and Wayland path blocks its thread until vsync (Windows is unmeasured), and a device rebuild blocks on `pollster` — today on the one owner thread every runtime shares | `renderer.rs:1751-1760`; the former `runner/device_recovery.rs:33`; ADR-0266 |
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
    domain: Arc<DeviceDomain>,          // ADR-0100: device and queue, shared with wgpu's callbacks
    painter: Option<WgpuPainter>,       // the shared content caches, taken for a frame
    pipelines: Option<PipelineLibrary>, // a torn painter's library, kept for the next (§7)
    slots: SlotTable<SurfaceSlot>,      // §3
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
- **The content caches live in the one painter every slot shares.** The context keeps one
  `WgpuPainter` (`crates/flui-engine/src/painter/mod.rs`) that holds the pipeline library, the
  glyph atlas, the image cache, the path caches and the chunk recordings derived from them
  (`crates/flui-engine/src/recording_cache.rs`). A frame takes the painter, is lent its slot's
  external textures, and puts it back. Holding the caches and the recordings in one owner is
  what keeps a recording from outliving the texels it names: whatever moves those texels
  advances the recording cache's `ResourceEpoch` (ADR-0304 §9), and a painter that is dropped
  takes its recordings with it. The headless renderer and the benches draw with a
  `WgpuPainter` of their own, so the caches keep one owning form.
- **The first format's pipelines are built while the first frame is.** The first slot to name a
  target format starts that format's `PipelineLibrary` on a worker thread
  (`crates/flui-engine/src/pipeline_warmup.rs`): a window slot at its attach, from the format its
  surface's capabilities give before any size is known; an offscreen slot at its attach; the
  headless renderer when it is made. The frame that first needs the library joins the worker, so
  the driver's compilation overlaps the application's first build instead of following it. A
  panic of the worker resumes on that frame, which settles `Retry` as any panicking frame does
  (§7), and the next frame builds the library itself; a device rebuild drops a warm-up under way
  (its library is the lost device's) and the first frame after it builds the new device's library
  itself. A first frame in another format than the warm-up's (a colour
  change before the first size, an offscreen slot of another format attached first) waits for the
  warm-up and builds its own format too. A second format, and the variants the shape and gradient
  caches build on demand, are built on the frame that first draws them. wasm32 has no thread to
  build on, and there the frame builds the library. Flutter's counterpart is Impeller, which
  creates its pipelines when its context starts, on its worker pool, where Skia's backend needed
  an SkSL warm-up captured by hand for each application; FLUI takes Impeller's shape, so an
  application captures nothing.
- **On Vulkan the pipelines are built through a `wgpu::PipelineCache` kept between runs.** A
  device is requested with `Features::PIPELINE_CACHE` where the adapter offers it, which wgpu
  implements on Vulkan alone (`wgpu-types/src/features.rs:1163-1172`), and every pipeline site
  creates through the context's cache (`crates/flui-engine/src/pipeline_device.rs`); elsewhere
  there is none. Its data lives in the application's machine-local storage under one name,
  `gpu-pipeline-cache` (`crates/flui-engine/src/pipeline_store.rs`), loaded by
  `GpuContext::load_pipeline_cache` before the first window attaches. The stored value is an
  envelope: a tag, the adapter's `wgpu::util::pipeline_cache_key` (vendor and device), the
  payload's length and a 64-bit `FxHasher` digest over key and payload (a crate the engine
  already has; the store has one writer, so the digest catches a torn or altered value, not a
  forger), refused above 64 MiB. wgpu checks
  only the payload's header, so a truncated, extended or altered value, or another adapter's, is
  the envelope's to refuse; a refused value is emptied at once and the context starts cold,
  without failing, and a driver update is wgpu's and the driver's to refuse inside the payload.
  The worker saves the cache after handing the warm-up's library over, off the frame path; the
  platform store stages and renames, so a crash mid-write leaves the previous value or none.
  Pipelines built later (another format, effects, variants built on first use) are not saved:
  the runners stop admitting storage writes before the context retires. The load waits a bounded
  time on the owner thread and past it starts a cold cache, which the warm-up still saves, since
  an IO pool may run on that thread (an application whose pool the owner drives never loads
  one). A
  lost device's data is not saved, and a rebuild starts a cold cache on the new device. This is
  what a driver without a disk cache of its own (Android's Vulkan drivers) needs between
  launches; lavapipe stores only a header in it.
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
  `EngineError::SurfaceReplacementNeeded`, and the slot's surface is `SurfaceHold::Wanted` (§6)
  until the owner sends one in a `Restore`. A `Restore` the backend cannot use makes it
  `SurfaceHold::Refused`, so the owner tells it from a surface dropped after one it took, and
  stops retrying that frame; one over the device's size limit keeps its surface, as after a
  rebuild, and the size it was configured at becomes `SlotSize::Refused` until a size that fits.
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
    RetryDevice { device: ProbedDevice },              // the application asked again (ADR-0267)
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
  with `StaleSlot`, as the former raster owner refused it with `AddressMismatch`.
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
- The former raster owner's `with_backend` is deleted; §12 maps each of
  its production callers to a command.
- A `SlotTarget::Offscreen` slot renders to a texture the executor reads back, so the headless
  renderer becomes a `GpuContext` with offscreen slots, on the path windows use
  (`crates/flui-engine/src/headless.rs:15`, `:104-109`), and the confirmation below runs in CI
  without a window.

§4. **A queued frame settles by its stamp.**

```rust
// flui-runtime
pub enum SubmitVerdict { Presented, NoPresent, NotShown, Retry, SurfaceStale, DeviceLost, Failed, Refused, Queued, QueuedReplacing(FrameEpoch) }
pub struct FrameSettlement { pub address: PresentationAddress, pub epoch: FrameEpoch, pub verdict: SubmitVerdict }
pub enum Settled { Applied, Stale }
pub trait FrameSink { fn submit(&mut self, epoch: FrameEpoch, publication: ScenePublication) -> SubmitVerdict; }
impl UiRuntime { pub fn settle_frame(&mut self, settlement: FrameSettlement) -> Settled; }
```

- Inline (§10), `FrameSink::submit` runs the executor's turn and answers the frame's own verdict,
  as today. Threaded, it answers `Queued` once the inbox has the frame, or `QueuedReplacing` with
  the stamp of the unrendered frame the new one replaced there; the UI runtime keeps the
  frame's post-submit work (the tree revision it commits, its input regions, its staged layer
  reads, its frame-aligned haptics, its presented timestamp) in a record keyed by the frame's
  address and epoch, captured at submit, so a settlement commits the frame it names and not the
  tree as it is when the answer arrives. Input regions publish when their frame settles
  `Presented`, so hit testing matches the pixels on screen; a frame that settles anything else
  leaves the regions of the last presented frame in force.
- The executor writes each settlement into the slot's record, never a queued verdict; the record
  holds every undrained settlement in a ring of the placement's depth, and the executor starts no
  frame of a slot whose ring is full, so none is overwritten. A frame superseded in the inbox never
  renders and gets no settlement: the UI runtime folds its record when the sink names its stamp
  in `QueuedReplacing` (from the sender's `SendReceipt::Replaced`), so the records a presentation
  keeps pending stay within the placement's depth and one. Then the executor calls the host's
  wake, with no lock held and contained (`RasterExecutor::turn`); the owner drains the record on
  its own thread and calls `settle_frame` after its own borrows are released (ADR-0287 §1).
- A settlement whose address or epoch no pending record holds is `Stale`, traced and ignored:
  `FrameEpoch` counts within one runtime's life, and the address carries the runtime's
  generational id.
- Every pending frame settles on every exit: `NotShown` while the slot has no surface or is
  hidden (§5), `DeviceLost` while the device is lost (§8), `Retry` after a contained panic (§7),
  `Failed` for each frame still pending when the executor stops or dies (§9). A detached slot's
  pending records leave with its presentation.

§5. **Pacing, restated per slot.** ADR-0045 decision 3 and the rule of decision 5 stand, per slot:

- `FrameClock`'s in-flight gate paces production: `FrameClock::poll` is handed the presentation's
  `FramesInFlight`, the frames it has pending (queued and not settled) against the limit its last
  frame's sink names,
  `FrameSink::in_flight_limit`. A presentation at its limit skips with `Backpressure`, keeps its
  demand and reports no pending work, and the settlement that frees a place wakes the loop. The
  limit is the placement's depth (§10): 1 inline, which never queues, 2 threaded. A slot whose
  surface its compositor paces (`FramePacing::Compositor`, ADR-0356) holds 1 in either placement:
  the frame callback that withholds the window's next frame is armed by the present, which a
  raster thread makes after the wake that queued the frame returned, so a second frame taken
  meanwhile would be presented in the same refresh as the first. wgpu's
  `desired_maximum_frame_latency` stays the slot's `FrameLatency` (ADR-0347 decision 4), set at
  `Attach` and changed by `Configure`, and stays decoupled from the depth; the GL backend ignores
  it as before.
- The slot calls its `SurfaceTarget::pre_present_notify` directly, immediately before
  `queue.present` and only for a frame that will present; `flui-engine` already reaches
  `flui-platform` (`crates/flui-engine/Cargo.toml:50-52`), so no hook closure is installed.
- `FallbackWake`'s deadline applies only when the slot's in-flight count is zero and its last
  settlement did not present. While a frame is in flight `UiRuntime::next_wake` keeps a floor, the
  next multiple of 50 ms since the oldest pending frame was handed to its sink, which the host
  turns into its `WaitUntil`; each window's frame wake drains the raster side's settlements,
  whatever its frame gate decides, so a lost wake costs at most one floor interval and a frame that
  never settles wakes the owner once per interval. The floor guards a settlement the raster side
  has not written: once it has settled every frame it holds of a window, its wake, which follows
  the settlement, has asked that window for the frame that drains them, and the window owes the
  loop no floor (`RasterHost::floor_owed`, `UiRuntime::next_wake_owing_floors`). Its host
  delivers that frame once the window can take one, as it does for a window drawn inline: a
  window its compositor does not draw, or has occluded, waits for its frame callback or its
  return, and the loop is not woken at the floor's pace to ask the other windows for frames
  they do not owe. A host that holds a window's frame back also wakes for any other wake
  deadline only once a window can take a frame (the Wayland loop's `FrameHold`, ADR-0356).
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
answered `None` for it in the former raster owner. Where the slot's surface and size stand is
one state, `SlotSurface { hold: SurfaceHold, size: SlotSize }`, which changes only through its
transition function under the inbox lock and which `SlotRecord::surface` publishes; the owner
decides from it (a new surface, another size, nothing until the window changes), and a failure the
slot holds is only what it reports. `SurfaceHold` is `Held`, `Wanted` (the slot dropped its
surface), `Restoring` (a `Restore` is sent and not answered, so the owner sends no second one) or
`Refused` (the newest `Restore` was refused); `SlotSize` is `Unsized`, `Pending`, `Applied` or
`Refused`. A minted size is `Pending`; the backend's answer to it counts only while no size was
minted since the one it answers, matched by generation, not by value. It makes the size
`Applied`, or `Refused` when the size is past the device's limit (`InvalidTargetSize` naming it),
and leaves it `Pending` on any other error or a panic, so the next turn retries it. A `Refused`
size is not retried: the slot keeps its previous configuration, frames stamped with that
generation settle `SurfaceStale`, and the executor warns once for that size until the owner sends
another one or a rebuilt device answers it. A size that is not settled (`Pending` or `Refused`) is
a barrier for every newer generation, and a restore or a rebuild authorises its generation only
after the backend's answer is in the state: neither a lost surface, a `Restore` nor a rebuild that
cannot configure the size authorises frames laid out for a size the slot does not have. The transition table is in `raster_executor/surface.rs`, and
`window_slots::each_surface_event_reaches_the_record` drives each row through the executor.
`GpuResourceGeneration`, `FrameStamp::gpu_resource_generation`,
the former `RasterHandle::bind_gpu_resource_generation` and
the former `PumpOutcome::ResourceOutdated` are deleted: a frame carries CPU data only, every GPU object it
needs is derived inside the executor, and the executor re-derives all of them after a rebuild
(§8), so no frame can be stale against a device. ADR-0037 §8 holds with the slot as the
presentation's raster owner.

§7. **A panic is contained per command, and costs the shared caches, not the pipelines.** The
executor runs each command under its own catch, in both placements.

- The panicking frame settles `Retry`; its payload goes into the slot's record, the first one kept
  and later ones discarded under containment (ADR-0281). The owner reports it at the slot's address
  through `UiRuntime::report_contained_panic` (ADR-0258 §1), and the next attempt waits out
  ADR-0258 §6's backoff, so a panic on every frame costs one attempt per backoff interval.
- The executor then drops the painter the frame unwound in, keeping only its pipeline library
  (`GpuContext::keep_pipelines`), and the next frame builds a painter on that library. What a
  panic can tear and other slots share goes with the dropped painter: the glyph atlas, the image
  and path caches, the scratch pools and the chunk recordings derived from them, so no recording
  outlives the caches it was derived from and none needs an epoch of the context's. A destructor
  in that drop that panics too costs the library as well; the frame's panic stays the one the
  slot's record gets. The panicking slot redraws in full; a slot whose retained scene the panic
  tore is rebuilt from the owner's next scene as ADR-0304 decides. The other slots keep their
  committed images, which only a completed commit changes (ADR-0100).
- The pipeline library is kept: an entry is inserted after its pipeline is built
  (`pipeline_cache.rs:551-552`), so a panic leaves a complete entry or none.
- A panic inside a submission quarantines the device domain, which is a device loss (ADR-0100;
  §8).
- User code the executor runs: the host's wake, `pre_present_notify`, a `tracing` subscriber, and
  the destructor of a dropped scene. None runs under the inbox lock. A `tracing` event inside a
  command can run while that command's slot is half-updated, which is why a panic there is
  contained and costs the caches above.
- A panic outside a slot's command (the host's wake, a trim, a destructor) is resumed on the
  owner once the turn is committed, inline. On a raster thread it cannot be resumed on the owner's
  thread: the thread contains it, keeps serving, discards the payload once its text is read, and
  hands the text to the owner as `ExecutorFailure::TurnPanicked`, which the owner reports as it
  reports a device's failure.

§8. **A lost device is the context's, and the rebuild keeps the surfaces.**

- wgpu's device-lost callback is installed once per device and marks the domain lost
  (`renderer.rs:1641`). Every slot's frames settle `DeviceLost` until the rebuild succeeds.
- One `DeviceRebuilds` per executor, which owns the one context, holds ADR-0267's backoff (16 ms
  doubling to a one-second ceiling) and its ceiling (ten failures at it) on the clock the executor
  is built with, so a manual clock walks it; a failed attempt's wait runs from when its answer
  was taken, since the driver may take hundreds of milliseconds to answer. The executor's record
  publishes the state as
  `DeviceHealth { Healthy, Rebuilding { attempts }, GivenUp { attempts } }`, which the host maps to
  ADR-0267's `DeviceRecovery { Pending, Failed, Abandoned }` (an executor's death is `Dead`, §9),
  with when the next rebuild may run
  (`RasterSender::device_retry_at`, for the host's wake) and why the last one failed
  (`RasterSender::take_device_failure`); that record is what `is_device_lost` reads instead of the
  backend (`raster_lane.rs:289`). A loss that this turn's own frames meet is published after the
  slots are served, with its first rebuild due now (`device_retry_at` answers the current instant),
  so the owner runs the next turn.
  The rebuild asks for its adapter on the executor's thread, since the adapter must present to a
  slot's surface and the request only enumerates the instance's adapters, and for its device on
  a worker of its own (`crates/flui-engine/src/device_worker.rs`), since a driver may take
  hundreds of milliseconds to answer after a reset. The turn that starts the request does not
  wait for it. The device stays lost meanwhile, and the owner's deadline (`device_retry_at`) is
  the request's own, `DEVICE_REQUEST_DEADLINE` (five seconds) after it started: a request
  unanswered by then is a failed attempt, so a driver that hangs inside it walks the backoff to
  the give-up instead of holding the device lost with no attempt counted, and its worker is left
  to end on its own. The worker's answer calls the executor's `DeviceAnswered` wake, which notes
  the answer in the device's record, so `device_retry_at` answers the request's start, a past
  instant, and then wakes the host from the answering thread: inline, the raster host asks every
  window it attached for a frame (every window lists its redraw requester), and the frame gate
  counts the due deadline, so that frame's turn installs the device; threaded, it rings the
  raster thread. Each request carries a `DeviceTicket` the executor mints; it installs only the
  answer to the ticket it waits for, and it stops waiting when the attempt fails or the streak
  ends (the device came back, an adoption, a give-up), so a late answer installs nothing, and a
  retiring context drops its worker's answer without waiting for it. A request that answers no
  device, or panics on its worker, is a failed attempt; the panic is resumed on the executor's
  thread when the answer is taken. A rebuild that panics counts as a failure and its panic is
  resumed once the turn has served every slot; the context counts its device lost until every
  slot is on the new one, so a rebuild that unwinds part way is asked for again from the start
  (what an unwound attempt refused is kept and answered by the attempt that finishes),
  and the executor restamps every slot whenever a lost device renders again, whichever way it
  came back. A backend whose device requests cannot be answered where the executor runs (the web,
  below) is given up
  at the first due attempt. After `GivenUp` the context stays given up until
  the application asks again; nothing retries on its own.
  That retry runs ADR-0244 decision 1's probe again in fresh instances, because the backend whose
  rebuilds exhausted the backoff may be the cause. The owner runs it (`GpuContext::probe`, over
  a window the platform still gives a surface; it blocks the owner thread and logs how long it
  took as `gpu_probe` on the `flui.gpu` target) and sends the device it answers in
  `RetryDevice { device: ProbedDevice }`, only for a device it read given up. The executor adopts
  every device it is sent, in order, so its instance is the one the owner's newest factory
  makes surfaces on. Adopting drops every window surface, since each was made on
  the instance the device's replaces, and restamps every slot as a rebuild does; the owner's
  factory is replaced by the probe's, and every window gets a new surface from it in a
  `Restore`. A probe that answers no device leaves the rebuild given up for the next request
  (`crates/flui-app/src/app/runner/raster_host/device_retry.rs`).
- A rebuild that succeeds mints each configured slot's next surface generation, so a frame
  stamped before it is refused as after a `Restore`; a size the lost device refused is `Pending`
  again, for the new device to answer, and a size not settled keeps its barrier
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
  `Restore`. A slot over the new device's size limit keeps its surface unconfigured, its size
  becomes `SlotSize::Refused` until a resize fits, as an offscreen slot's does. Each slot redraws in full
  and re-uploads its external textures from the newest image of each that it keeps
  (ADR-0233).
- On the web the rebuild is a future the owner spawns: the lost context is retired with its slots
  first, so no two contexts present to one canvas, and no frame runs until the new context has
  attached the canvas's slot. Each slot's kept texture images go to the slot attached for its
  window next, and the driver's report of a lost device wakes the owner without waiting for new
  content. A loss is a rebuild the owner owes, asked again at each animation frame and never given
  up, since the page has no `retry_render_device` to ask again with; the application is not told
  of builds that keep failing.

§9. **Teardown is bounded and the executor's health is typed.** ADR-0045 decision 7 stands per
executor and slot.

- `ExecutorHealth { Running, ShutDown, Died }` rides the executor's record; the two non-running
  states log at `error`. Once it is not `Running`, every send answers `Gone` with its command, and
  the owner settles every frame still pending in the inbox of a slot it has not detached `Failed`,
  since the executor no longer can; a detached slot's frame leaves with its presentation,
  unsettled, as the executor's own detach drops it. A slot settles in order even then: the frame
  a wedged render holds settles only if that render returns, and a frame queued behind it in that
  slot settles `Failed` after it, so a late `Presented` never arrives for a frame whose record a
  later settlement already folded. A dead executor makes no backend call once the call it died
  during returns, except that its stop still takes each slot down in the backend:
  that stop settles or drops what the inboxes hold.
- `Detach` and `Release` complete before the native window is destroyed or its surface given
  back. Inline they complete within the call. Threaded, the owner waits for the slot's record with
  a deadline (`DETACH_DEADLINE`, two seconds, above wgpu's one-second acquire timeout), with no
  borrow of the host or a runtime held. It waits the same way for an attach's answer, inside the
  host's borrow, and for the thread's stop wherever the handle drops; a window procedure a sent
  message reaches during those two may run with the host borrowed (`docs/gaps.md`). It waits
  through the platform's `flui_platform::OwnerWait` where the platform
  has one (`Platform::owner_wait`), which the thread notifies after each turn and once it
  stopped, and on a condition variable otherwise. On Win32 that wait serves sent messages only
  (`MsgWaitForMultipleObjectsEx` with `QS_SENDMESSAGE`, then `PeekMessageW` with
  `PM_QS_SENDMESSAGE`): a present can send a message to the window's thread, and a message thread
  that waits on the render thread without serving it deadlocks (Microsoft's DXGI guidance, below).
- On a timeout the window is quarantined, not closed, and the executor is marked `Died`. A raster
  thread wedged past the deadline holds the context, so every window stops drawing, not one: the
  owner asks every open window for a frame, and each window's wake settles the frames its slot
  still holds `NoPresent` (finished, nothing shown), so its UI runtime commits the tree they were
  painted from and the input held for that commit replays. Any settlement it drains after the
  death that shows nothing (the `Failed` the death gave an inbox frame, a stale or withheld one)
  answers `NoPresent` too, a frame submitted afterwards answers
  `NoPresent` without reaching the executor, and the host reports no rebuild or restore deadline
  a dead executor left. Each window tells its UI runtime once
  (`DeviceRecovery::Dead`, reported through the frame-failure handler as a lost device): the
  presentation keeps building and laying out its frames, which the host answers without drawing,
  so it keeps taking input, lifecycle changes and the agent's requests and its semantics tree
  follows the application; the death releases a given-up rebuild's hold and asks for a frame,
  which commits a tree that a frame dropped before the death left uncommitted. The failure itself
  (`ExecutorFailure::Wedged`, or a turn's panic) is taken from the sender once
  (`take_executor_failure`) and logged, with no panic
  on its thread. A window closed while the thread does not answer its detach keeps its native
  window and its surface target until the process exits, since the wedged thread may still
  present over them. Once the thread is known wedged (a detach or an attach it did not answer), the owner calls
  `SurfaceTarget::keep_past_close` on every window it served, open ones included, and a close by
  the user, a command or the exit then hides the native window instead of destroying it, so the
  window is closed to the application
  (no handler, in no window list, no input) while what its handles name lives on.
- `Died` is terminal for the application's rendering, as a dead runtime is under ADR-0258 §2: the
  owner does not bootstrap a second context while the wedged thread still holds the first and its
  quarantined windows, `retry_render_device` leaves a dead renderer as it is, and the application
  decides whether to exit.

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
    pub fn spawn(                                                 // threaded: a named raster thread
        backend: B,
        clock: Arc<dyn MonotonicClock + Send + Sync>,
        wake: impl Fn() + Send + 'static,
    ) -> std::io::Result<(ExecutorHandle, RasterSender)>;
}
```

- Inline, the owner holds the executor and runs its turn from `FrameSink::submit` and after each
  lifecycle command. Threaded, `spawn` moves the backend and its context onto one raster thread,
  which builds the executor there, and the owner holds only an `ExecutorHandle`; one raster thread
  serves every slot (ADR-0091 §4). `spawn` is a constructor, not a method of an inline executor:
  the inline executor keeps its clock as an `Rc` and its wake as a plain closure, which stay on
  the owner's thread, and the backend is asked `Send` alone, not `Sync`. The thread turns once per
  ring of a doorbell, which every accepted send and every drain that frees a ring's place rings,
  and once when a lost device's rebuild is due.
- The target placement is ADR-0091 §5's table: threaded on Win32 and on Linux (Wayland and X11);
  inline on macOS (wgpu-hal's Metal acquire messages `NSWindow` from the acquiring thread,
  `wgpu-hal/src/metal/surface.rs:354-370`) and on the web; inline on Android and iOS until a
  device run meets the budget; inline in headless and testing. A platform becomes threaded when a
  run on it meets ADR-0091 §5's pacing budget, including a two-window case with one window
  occluded; `docs/platform-parity.md` records the run. The occluded case is part of the budget
  on every platform, inline ones included, because one executor serves every window in both
  placements.
- Threaded, the owner's host waits for the thread's answer to a window's attach
  (`ExecutorHandle::await_attach`), so a window the thread refuses is refused at its attach as it
  is inline; a thread that does not answer within the detach deadline is wedged at the attach as
  at a detach (§9). Then, or when the thread died of anything else before it answered, the window
  is not refused, since the thread or its queue may still hold its surface;
  it is quarantined as §9 says. It answers a frame `Queued`, or `QueuedReplacing` with the epoch the
  slot's `SendReceipt::Replaced` names, keeps what each held frame's verdict needs (its version,
  surface and retry), and its sink's in-flight limit is the slot's (§5): the placement's depth,
  or one where the compositor paces the surface. A settlement
  carries whether a call on its slot panicked since the previous settlement and the owner has
  not taken it (`SlotSettlement::panicked`), which is how an owner that drains later
  than the turn tells the frame a slot's panic came with; a slot holds a panic in place of a
  refusal held before it, since the owner reports the one and only logs the other; a frame the
  owner held unsettled when it took a panic, one that replaced it, and one drawn after a panicked
  one of the same drain have their base forgotten; the owner takes a slot's panic with its
  settlements under one lock (`RasterSender::drain_settlements_and_failure`) and forgets the base
  after a drain that took one,
  so the window's next frame draws in full. A surface whose
  resize or upload panicked gets one retry, counted by surface (size and generation) rather than by
  frame: the first frame that settles stale on it with the slot's panic (marked, or held unsettled
  when the owner took that panic, or replacing one that was) is retried and any later one refused,
  until a frame of the window settles any other way than a panicked render. The thread's
  wake asks every attached window for a frame through its
  `RedrawRequester`, each one contained; each window's wake drains its own slot's settlements
  into `UiRuntime::settle_frame` first, before its frame gate and whatever the wake goes on to do,
  and holds a panic of the code they ran for the end of that window's wake. Neither the window's
  first-reveal fallback nor its pacing fallback arms while the thread holds a frame of the window:
  its settlement does, a presented frame revealing the window and one that presented nothing
  counting toward the reveal's fallback as a frame of the wake would. A window's detach waits for
  the thread (`ExecutorHandle::detach`) once the host's borrow is released, or at once through the
  host's link to the thread when it meets the host busy, since the window goes when the detach
  returns.
- The desktop loop places its executor on its raster thread on the Wayland host, in every build
  (`desktop_placement` in `crates/flui-app/src/app/runner/raster_host.rs`), by the run the
  Confirmation records. X11 stays inline until a run on a real X server meets the budget: Xvfb
  has no vsync, so its pacing could not be judged, and an unmapped window's `Fifo` present may
  block (`docs/gaps.md`), which on the one raster thread would hold every window's frames.
  Windows and macOS stay inline until a run on each meets the budget; the headless host and
  every run of `flui_app::testing` stay inline. No switch chooses the placement.
- `render_lent` serves the hot-reload plugin's scene, lent by reference for the length of one
  call (`ScenePlugin::try_render_frame` in `crates/flui-app/src/app/hot_reload.rs`, through
  `RasterHost::render_lent` on Android).
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
(`crates/flui-engine/src/raster_executor/backend.rs`); only `spawn` asks for `Send`.

§12. **The lane holds no lock, and nothing reaches past the commands.** `Arc<Mutex<RasterLane>>`
(`desktop.rs:245-252`) and `LaneStamp`'s mutex (`raster_lane.rs:72-81`) are removed with their
reason, which no longer holds: the callbacks they serve take `'static` closures with no `Send`
bound (`window_routes.rs:185-237`, `window_install.rs:269`). The owner side is an owner-local
`RasterHost` holding the factory, the placement and each slot's handle, in an `Rc<RefCell<_>>`; a
re-entrant frame dispatch that finds it borrowed answers `SubmitVerdict::Retry`, as the
`try_lock` does today (`raster_lane.rs:486-490`). On the web, `Arc<Mutex<Option<Renderer>>>`
(`crates/flui-app/src/app/runner/web.rs:79`) and its direct sink (`raster_lane.rs:513`, `web.rs:325`)
are removed for the inline executor. No `unsafe impl Send` is added and no `Send` position joins
the facade. Every production use of `with_backend` becomes:

| Call site | What it does | Becomes |
|---|---|---|
| the former `runner/surface_lifecycle.rs:144` | follows the display's colour | `Configure { change: Color(..) }` |
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
  - `the_first_slots_attach_builds_its_pipelines_on_a_worker_once`,
    `a_warm_up_that_panics_fails_the_frame_that_joins_it_and_the_next_builds_inline` and
    `a_device_rebuild_drops_a_warm_up_under_way` and, in `headless.rs`,
    `a_new_renderer_warms_its_capture_pipelines_and_the_first_capture_takes_them` (the warm-up of
    §1), with the cold first frame timed by `crates/flui-engine/benches/first_frame.rs`;
  - the persisted cache of §1: `pipeline_store::tests::bytes_the_envelope_does_not_prove_are_refused_with_their_reason`,
    `a_corrupt_stored_pipeline_cache_is_emptied_and_the_context_starts_cold`,
    `a_finished_warm_up_saves_the_pipeline_cache_for_this_adapter`,
    `a_saved_pipeline_cache_seeds_the_next_context`
    and `a_device_rebuild_restarts_the_pipeline_cache_cold`.
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
  `tests/main.rs`: `a_queued_frame_settles_by_its_stamp_and_a_stale_settlement_is_ignored` and
  `a_sink_holding_its_depth_gets_no_frame_until_one_settles` (the in-flight gate of §5), with
  the floor of §5 in its `frame_failure_containment_matrix` case
  `a_frame_in_flight_bounds_the_owners_sleep`.
- Conformance, to be added in `flui-native`: `a_surface_targets_display_equals_the_platforms_connection`,
  one row per host, in `flui-native`'s Linux suites: the Wayland row under a headless `weston`,
  the X11 row under `Xvfb`, the winit and headless rows without a display server.
- The raster protocol's tests, per slot, in `crates/flui-engine/tests/raster_executor.rs`:
  `latest_frame_wins_and_superseded_damage_folds_into_the_survivor`,
  `stale_surface_generation_is_rejected_before_render`,
  `trims_coalesce_to_the_strongest_level_and_apply_before_the_frame`,
  `every_render_outcome_settles_its_frame`,
  `a_slot_that_is_not_shown_settles_not_shown_and_redraws_in_full_when_shown` (a hidden slot is
  not drawn, so no surface texture is acquired for it),
  `a_stopped_executor_settles_every_pending_frame_failed`,
  `a_full_settlement_ring_holds_the_next_frame`,
  `a_full_lifecycle_queue_returns_the_command` and
  `a_command_sent_during_the_stop_comes_back_gone`; on the raster thread, in
  `crates/flui-engine/tests/raster_thread.rs`, `a_raster_thread_that_does_not_answer_a_detach_dies_at_its_deadline`
  `a_frame_queued_behind_a_wedged_render_settles_after_it` and
  `a_frame_whose_resize_the_executor_died_during_is_not_rendered` and
  `a_scene_dropped_at_the_death_panics_into_its_own_slot` (§9),
  `the_owner_waits_for_the_threads_answer_to_an_attach`,
  `a_raster_thread_that_does_not_answer_an_attach_dies_at_its_deadline`,
  `a_frame_that_panicked_settles_marked_so`, with
  `crates/flui-engine/tests/raster_executor.rs`'s
  `a_texture_panic_after_a_failed_resize_is_the_failure_held`,
  `a_panic_takes_the_place_of_a_refusal_held_from_an_earlier_turn` and
  `a_panic_marks_the_next_settlement_until_the_owner_takes_it` (§10); on the host's threaded
  placement, in `crates/flui-app/src/app/runner/raster_host/tests.rs`,
  `a_threaded_frame_is_queued_and_settles_through_the_windows_drain`,
  `a_threaded_frame_replaces_the_one_waiting_behind_a_render`,
  `a_threaded_windows_detach_returns_once_its_slot_is_down`,
  `a_window_the_raster_thread_refuses_is_refused_at_attach`,
  `a_window_whose_attach_the_raster_thread_does_not_answer_is_kept`,
  `a_window_wake_settles_what_the_raster_thread_finished`,
  `a_compositor_paced_window_holds_one_frame_on_a_raster_thread` (§5, with
  `crates/flui-app/tests/native_linux.rs`'s
  `an_app_whose_window_is_never_drawn_still_quits` on the Wayland host's raster thread),
  `a_threaded_frame_that_did_not_present_counts_toward_the_first_reveal` and
  `a_wedged_raster_thread_quarantines_its_open_window` (§9), with the pacing it leaves in
  `crates/flui-app/src/app/runner/frame_pacing.rs`'s `a_held_frame_arms_no_fallback` (§10);
  ADR-0244's
  `gpu_backend_fallback_order`; and
  ADR-0267's `a_frame_dropped_on_a_lost_device_waits_out_the_backoff`,
  `a_failed_rebuild_is_reported_with_the_dropped_frame`,
  `a_given_up_rebuild_holds_frames_until_the_application_asks_again`,
  `a_lost_device_waits_out_its_rebuild_backoff`,
  `a_dead_renderer_is_reported_once_and_its_frames_go_on` (§9) and
  `a_given_up_device_renders_again_when_the_application_asks`; the device request off the turn
  (§8): `a_device_request_answered_after_its_turn_is_installed_by_the_next`,
  `a_device_request_unanswered_past_its_deadline_fails_its_attempt`,
  `an_answer_nobody_waits_for_installs_nothing` and
  `a_failed_device_request_waits_its_backoff_from_its_answer`, with `device_worker.rs`'s
  `a_worker_hands_over_its_answer_or_its_panic_and_wakes_the_executor` and
  `a_request_nobody_waits_for_holds_nobody`; the raster host's
  `an_inline_device_request_asks_the_windows_for_the_frame_that_installs_it`, and `gpu_context.rs`'s
  `a_slot_detached_while_its_device_is_requested_is_not_served_by_the_new_device`.
- Runs: a platform's placement becomes threaded only with its run recorded in
  `docs/platform-parity.md` (§10, the row `frames-drawn-on-a-raster-thread`). Wayland is threaded
  on this run of two windows, one occluded, on lavapipe and on no device:
  - Wayland under Weston: threaded, a median interval of 26.6 ms (p99 29–31 ms) and none of
    1108 intervals under 5 ms; inline, a median of 26.5 ms. The compositor runs at about 26 ms.
  - The raster share: an update frame takes about 0.3 ms on the owner's thread and 0.7–4.3 ms of
    raster CPU (`crates/flui-app/benches/raster_share.rs`).

  X11 under Xvfb, which has no vsync, did not judge its pacing: there the occluded window did
  not slow the visible one (threaded 1.13/1.43/11.0 ms median/p90/max, inline
  1.71/2.08/24.1 ms), and X11 stays inline until a run on a real X server.
- The floor of §5: `crates/flui-app/src/app/runner/raster_host/tests.rs`'s
  `a_window_the_raster_thread_settled_owes_no_floor_beside_one_it_renders`, and for any
  deadline on a host that holds frames back, `flui-native`'s `wayland_scripted` case
  `a_moving_wake_deadline_polls_no_window_the_compositor_never_draws`.

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

The code moves in this order, each a change of its own and the inline placement first, because it
shipped first on every platform: the context and the executor inline, with the headless
renderer on offscreen slots; the desktop, Android and iOS runners on slots, with `with_backend`,
the lane's lock and the hook closure removed; the web on the inline executor, with its direct sink
removed; the settlement path in the UI runtime; then the threaded placement on Wayland, X11 and
Win32, each when its run meets the budget.

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
