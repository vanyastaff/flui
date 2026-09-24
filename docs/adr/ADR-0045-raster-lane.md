# ADR-0045: The raster lane — threaded raster ownership, shared GPU services, and what paces production once present stops blocking the produce loop

- **Status:** Proposed — accepted when surface acquisition and present run on the raster thread and
  a CI frame-pacing budget stays within budget.
- **Date:** 2026-08-07
- **Amended by:** ADR-0058 (decision 3), ADR-0063 (decision 1, §7)
- **Related:** ADR-0027 §5–§6, ADR-0037, ADR-0039 §3, ADR-0044 §7 and §9; issue #559

## Context

ADR-0027 §5 designed the raster boundary — an owned `SceneSnapshot` moving down, typed acks moving
up, a coalescing latest-frame-wins mailbox, a one-shot shutdown — and deferred one question: which
thread the raster owner loops on. The shipping baseline is a synchronous in-process owner behind
that seam, with a threaded harness keeping the protocol honest. ADR-0044 §9 added the in-flight
ticket and the retire→wake edge, and stated its own boundary: no production `FrameClock` reads a
live raster owner, so `poll` never returns `Skip(Backpressure)` in production.

Every production runner holds the backend behind a lock (`Arc<Mutex<Renderer>>` on desktop, Android
and `run_direct`; `Arc<Mutex<Option<Renderer>>>` on web, where the `Option` means "adapter not
ready"), and a second mutator — the resize applier — reaches through the same lock at an arbitrary
point relative to paint. So this record introduces a generation scheme into the shipping path
rather than repairing one; the generation decision (4) lands before anything else touches
generations.

Concurrency and presentation architecture are leapfrog zones (ADR-0027). Flutter's raster thread
is a reference for shape, not a behavioral oracle for any decision below.

## Decision

### 1. The split rule, and what crosses

> **Owner-affine** = reads or mutates the element/render tree, demand, scheduling or input state.
> **Raster-affine** = touches a `wgpu::Surface`, its configuration, a swapchain texture, or GPU
> submission. Exactly one value crosses down — an owned `SceneSnapshot`. Only `Copy` outcome facts
> cross up. **The owner thread never blocks on the raster thread.**

Tie-breakers, in order: if leaving a fact on the UI thread would force a GPU block, it is
raster-affine; if moving it would force the raster thread to read the tree, it is owner-affine;
otherwise owner-affine, with no carve-outs.

**The line runs through the surface's lifecycle:** creating or recreating a `wgpu::Surface`
(including the device-loss rebuild) is owner-affine; *using* one — acquire, present,
configure/resize, present-mode selection, damage marking, present timestamping — is raster-affine.
`PresentationState`, `FrameClock`, `UpdateScheduler`, gestures/focus/IME, the demand mask,
`wake_frame` and `SceneSnapshot` construction stay on the owner.

**Why there: one platform rule.** AppKit UI objects (`NSView`, `NSWindow`) may be messaged only from
the main thread, which on macOS is the owner thread. For surface creation this is a hard panic:
`wgpu-hal`'s Metal `create_surface` calls `raw_window_metal::Layer::from_ns_view`, which opens with
`MainThreadMarker::new().expect(...)`, and `raw-window-handle` proves `RawWindowHandle: !Send` at
compile time. `configure` is off-main-safe — it touches only the retained `CAMetalLayer` — and
`wgpu-hal` documents `Surface::dimensions` as safe off the main thread. The same rule decides three
things, each stated once: surface creation is owner-affine (here); `request_redraw` is never called
from the raster thread (decision 5); window destruction is never performed by the raster thread
(decision 7). It is one rule on every platform; macOS merely enforces it at runtime (Win32 handles
are plain integers; winit's X11 connection is `XInitThreads`-initialised and `Send + Sync`).

**macOS runs `RasterMode::Inline` until upstream changes.** `wgpu-hal`'s Metal `acquire_texture`
carries a macOS occlusion workaround that walks to the hosting `NSWindow` and sends it
`-occlusionState` from whatever thread acquires — the raster thread under a threaded lane. It does
not panic, but upstream's own `display_hdr_info` in the same file gates the same class of access on
the main thread and calls off-thread use undefined behavior. Moving acquisition back to the owner
would delete the point of the lane, so macOS joins wasm and the hot-reload plugin path on the
inline lane. Reopen condition: a `wgpu-hal` release whose Metal `acquire_texture` no longer messages
`NSView`/`NSWindow` off the calling thread, or an upstream statement that it is safe, verified in
the locked version. Nothing automated watches either disjunct: the first is checked by reading
Metal's `acquire_texture` in the `wgpu-hal` being adopted, and the second is prose and needs a
human to read it. On macOS
the `Occluded` signal decision 3 relies on is produced by that very block, so there the analysis
describes the inline lane.

**The layout size moves to the owner.** Layout reads the surface size back out of the backend
(`renderer.size()`) to build the root constraints. Under the rule it may not, so
`PresentationState` gains an owner-affine surface-size cell written by the platform resize path and
read by layout.

**The `Send` posture.** The `Renderer` moves into the thread once, at lane construction — no `Arc`,
no `Mutex`, no lock in the public API. `RasterBackend` has a `Send` supertrait, the spawn entry point
requires `B: Send + 'static`, and `dyn RasterBackend + Send` stays object-safe. `Renderer: Send` is
ordinary auto-derivation, so a future `!Send` field is a compile error rather than a silently
widened assertion; `!Sync` is unchanged. The blanket `unsafe impl Send for Renderer` this record
originally planned to delete together with the saved raw handles was deleted earlier, independently
of the lane, by ADR-0063: the renderer owns its surface target (`WindowTarget`, held in a
`SurfaceLease`) and never saves raw handles. Retaining the target inside `Renderer` is transitional;
the lease moves to the presentation when decision 2's windowed `GpuServices` constructor lands.

### 2. Shared GPU services are per owner thread, not per process

Every `Renderer` builds its own `Instance → Adapter → Device → Queue`. Three hazards shape sharing:
`set_device_lost_callback` is last-writer-wins; `recover()` replaces the device wholesale, so a
sibling holding the old `Arc<Device>` renders into a dead device; and the per-surface caches embed
an `Arc<Device>`.

**Decision:** a `flui_engine::GpuServices` holding instance/adapter/device/queue/capabilities, the
shared `ShaderCache`, one `device_lost: Arc<AtomicBool>` behind exactly one callback install, and an
immutable `GpuResourceGeneration` (prefixed because `flui_foundation::epoch::ResourceGeneration`
already names worker-cache freshness). It lives in `flui-engine`, which owns all wgpu state.

- **Status of the type.** An offscreen-only `GpuServices` shipped first and was deleted: it had no
  production consumer, and a public constructor no windowed caller could reach is a misleading API.
  It returns together with `ReplaceServices` (below), with `flui-app`'s `DeviceRecovery` seam
  re-pointed at the owner thread in the same change. Until then `Renderer::new` is the advertised
  entry point.
- **The windowed half is a factory, not a holder.** `Adapter::request_adapter` resolves a
  `compatible_surface` against the *receiving* instance's registry, so instance and surface must be
  created together — the shape `Renderer::build_windowed_gpu_stack` already has.
- **Scope is one owner thread.** `AppRuntime` is `thread_local!`, so two owner threads get two
  devices. That is checked (a debug-only `created_on: ThreadId` assert), not just documented.
  Process-level sharing waits for a second owner thread with a real GPU workload.
- **The slot is re-settable** (an interior-mutable `Option`, not a `OnceCell`), because device-loss
  recovery *is* re-setting it.
- **Adapter selection** stays surface-derived: the first window picks, later windows inherit, and
  the choice is permanent for the process. Re-selection on display-topology change is out of scope.
- **Only `ShaderCache` is shared.** Texture/path caches and the glyph atlas are per-surface and
  mutated every frame. Revisit at three or more simultaneous surfaces and measured duplicate atlas
  residency ≥ 32 MiB or duplicate pipeline compiles ≥ 50 ms at startup.
- **Recovery is a whole-owner-thread event.** The device-lost observation crosses threads; the
  recreation runs on the owner thread, resolved by a plain generation compare (not a CAS).
  `AppRuntime::recreate_gpu(observed)` mints one new `GpuServices` and sends every lane on that
  thread `ReplaceServices { services, surface, generation }` on the ordered command path. The owner
  builds each replacement surface (the lane may not); the lane drops its per-surface caches
  wholesale, configures the new surface, and mints the new `SurfaceGeneration` through decision 4's
  counter. Between loss and replacement, frames stamped with the old `GpuResourceGeneration` are
  rejected on the resource axis, then frames stamped with the new one are rejected on the surface
  axis until the lane applies the command — rejected, not queued. Cost: one window's device loss
  blanks every window on that owner thread for at least a frame.

### 3. What paces production once the produce loop stops blocking

The blocking `Fifo` acquire/present paces production only while the raster owner runs inline on the
produce thread (ADR-0058). Under a threaded lane both calls move to the raster side and the produce
loop reaches neither. Production is then paced by:

- `FrameClock`'s in-flight capacity gate (`Skip(Backpressure)`, live in production for the first
  time — closing ADR-0044 §9's boundary) and `min_produce_interval`;
- the platform's own pacing signal, armed before every present through the pre-present hook
  (`RasterBackend::set_pre_present_hook`, ADR-0058 decision 1) — the raster backend fires it
  immediately before `queue.present`, for a frame that will present only;
- for frames that ran the pipeline but did not present, a `FallbackWake` **deadline** delivered
  through the wake-deadline hook (`ControlFlow::WaitUntil`, ADR-0044 §7; ADR-0058 decision 2).
  **Never a sleep:** the event-loop thread does not block.

**The fallback deadline applies only when `in_flight == 0` and the last completion did not
present.** "No completion drained this cycle" is not a substitute: the drain is empty exactly when
the GPU is the bottleneck, so treating it as "nothing presented" would defer the UI more the slower
the lane gets. While `in_flight > 0` the lane owes a frame, and the backpressure gate is the pacer.

**The presented bit rides the coalesced reliable slot**, not the ack lane. `RasterAck` is
deliberately lossy and drops the *newest* ack on a full channel — exactly the sample the predicate
reads. The `SurfaceState` slot that already carries `required_generation` and `device_lost` gains
`last_completion: Option<{ epoch, presented }>`, written by the pump on every retire; a coalesced
latest-wins slot cannot lose the latest value. The completion ring stays the histogram's source.
The predicate is sound only because decision 5's floor guarantees a lost wake cannot strand
`in_flight > 0` forever.

**The occluded window.** On `CurrentSurfaceTexture::Occluded` acquisition skips without blocking,
a running ticker keeps re-marking demand (ADR-0044 §1), and no produce interval is configured by
default — so nothing bounds the loop unless a bound is designed in. After the move that bound is the
fallback deadline above, not something inherited. winit has no Wayland `Occluded` emitter; there the
compositor withholds frame callbacks from a hidden surface once the pre-present hook arms them,
while Windows/X11 keep delivering redraws and rely on the deadline.

**Exit criterion.** This record is Accepted when surface acquisition and present run on the raster
thread and a CI frame-pacing budget (median / p90 / max inter-present interval against the
display's native period, compared with the still-serial baseline ADR-0058 measured) stays within
budget. An argument that the clock gates "should" pace is not evidence.

### 4. One `SurfaceGeneration` counter per lane, owned by the mailbox

Threading the lane puts `SurfaceGeneration` mint sites on two threads: resize and attach/detach
(owner), the surface-lost bump (raster), and surface recreation during recovery.

**Decision: exactly one counter per raster lane, owned by the mailbox state and mutated only under
its lock.** Every mint routes through it. The owner half's `current_surface_generation` becomes the
generation of the currently applied configuration, assigned from the value carried with the command
it applied; it is never independently incremented.

Two counters "reconciling on the next read" fail both ways:

- **False accept.** Both counters at G. The surface is lost mid-render; the raster side bumps to G+1
  unobserved. The owner resizes, mints G+1 on its own counter, and stamps a frame G+1 produced
  against the pre-resize configuration. The pump compares G+1 == G+1 and renders a stale frame
  against the post-resize configuration.
- **Permanent starvation.** The raster counter runs ahead through repeated losses and every frame
  is rejected forever.

Lock order is unchanged: mint under the mailbox state, release, then write the surface state.

**Liveness during a drag resize needs no handshake.** The mint is generation-forward — `resize`
returns the generation it minted, and the resize event and frame request dispatch through the same
owner-thread entry point, so stamping and submitting cannot interleave with resize handling. A
request/ack handshake would block the owner while the raster thread is parked in acquisition, at
drag event rates.

**Both axes are checked, and `ZERO` means "no surface".** A frame renders only if its
`SurfaceGeneration` equals the applied configuration's generation, its `GpuResourceGeneration`
equals the current services' generation, and the surface is attached. `SurfaceGeneration::ZERO` is
rejected outright, replacing web's `Option<Renderer>` with typed data. Caches are invalidated by
ownership, not keying.

The rule is single-*counter*, not single-mint-site (one mint site would be false the first time
a surface is lost):

> `SurfaceGeneration` is minted by exactly one counter per raster lane, owned by the raster mailbox
> and mutated only under its state lock. Both the owner half and the handle half mint through it; no
> component keeps a private counter. A frame renders only if its stamp equals the generation of the
> applied configuration.

Its evidence is the test that mints from the surface-lost path, resizes, then submits a frame
stamped with the pre-resize generation and requires it rejected.

### 5. The wake never crosses to the platform on the raster thread

The raster side never calls `PlatformWindow::request_redraw`. Its wake hook pushes onto a channel
and pokes a relay; the platform's event-loop waker drains it on the owner thread. Either ground is
sufficient: a raster-thread call would falsify `MacOSWindow`'s `unsafe impl Send` precondition
(decision 1's rule), and the wake hook is `Send + Sync`-bound while realm state is `!Send`, so
capturing owner state in it does not compile.

The relay is a new verb on ADR-0039 §3's `PlatformProxy` lane, not a second wake path. Only winit
implements a real transport today; Win32 (`PostMessageW` to a message-only HWND), AppKit
(`CFRunLoopSource` in a common run-loop mode) and headless (flag plus pump) install
`ClosedTransport` and need real work. Web runs inline.

**A missed wake costs time, not liveness.** Every poke can fail, and Win32/AppKit live resize runs a
nested modal loop. While `in_flight > 0` the owner loop installs a `ControlFlow::WaitUntil` floor
through ADR-0044 §7's wake-deadline hook, so a lost poke costs at most one floor interval. Each
backend states at its implementation site what a failed poke does there.

### 6. `PipelineDepth` replaces `max_frames_in_flight`

With a capacity-one mailbox and a gate-honouring producer, at most two tickets are live (the pending
slot and the one being rendered); threading does not widen that. A `NonZeroU8` advertising
`1..=255` was a lying API. `RasterOptions` — the field's carrier, read only by its own tests — is
deleted outright. `PipelineDepth { Auto, NoOverlap, Overlap }` (`#[non_exhaustive]`), resolved by a
pure `select_pipeline_policy(mode, present_mode, caps)`, arrives with the threaded lane; the
topology is named `RasterMode { Inline, Threaded }`. `Overlap` on an `Inline` lane is clamped to
`NoOverlap` with a warning.

| Input | Resolves to |
|---|---|
| `Threaded` + `Fifo` | `Overlap` (2) — the produce thread stops blocking on vsync |
| `Threaded` + `Mailbox`/`Immediate` | `Overlap` (2) — the clock throttle is the only pacer |
| `Inline` (same-thread pump, wasm, diagnostic) | `NoOverlap` (1) |
| GL backend | as above; wgpu ignores `desired_maximum_frame_latency` there |

The clock-side threshold and wgpu's `desired_maximum_frame_latency` (a single source literal; see
ADR-0058 decision 0) stay decoupled. At depth 2 with one producer a pending frame is never
superseded, so `latest_frame_wins_supersedes_pending` stays covered only by the threaded harness.

### 7. Lane health, and bounded teardown

`LaneHealth { Running, ShutDown, Died }` rides the coalesced reliable slot; the two non-running
states log at `error`. `Died`: `run_until_shutdown` takes `self` by value, so an unwind drops the
owner and `submit` returns `OwnerGone` (corroborated by `JoinHandle::is_finished`). `ShutDown` vs
`Died` is whether the shutdown one-shot fired.

**Teardown is bounded.** `JoinHandle::join` has no timeout, so the lane's `Drop` sends shutdown and
waits on the one-shot with a deadline. On timeout it detaches with a loud error, and **the window is
quarantined, not closed**: the presentation leaves the runtime and its
`(JoinHandle, Arc<dyn PlatformWindow>)` moves into a process-lifetime quarantine; `close()` is never
called on it and the OS reclaims both at exit. `ExitPolicy::OnLastWindowClosed` keys on realm slots,
so exit is unaffected. A best-effort `minimize()` is cosmetic.

Why quarantine is the only sound outcome: a wedged thread is typically blocked inside
`get_current_texture()` on a surface built from that window, and on Win32 and AppKit the
`Arc<dyn PlatformWindow>` does **not** own the native lifetime — `DestroyWindow` / `[NSWindow close]`
run regardless of Rust owners — so the surface must never see its window closed. The surface cannot
be dropped first (the thread is blocked on it; wgpu offers no cancellation), and the raster thread
cannot destroy the window later (window destruction is thread-affine on Win32 and AppKit).
Flutter's unbounded form of this wait — `Shell::OnPlatformViewDestroyed` latching the platform
thread until raster releases the surface — is an ANR in production (flutter/flutter#190599,
#169585); the bounded wait plus quarantine is the improvement. The cost: a wedged raster thread
leaks a native window as well as a thread.

## Consequences

**Positive**

- The produce path stops blocking on the GPU; produce and present overlap and the backpressure gate
  becomes reachable in production.
- The lock-wrapped backend sites are deleted, and resize becomes an ordered command on the lane.
- One `wgpu::Device` per owner thread, one device-lost callback, one shader cache.
- Stale-frame rejection, detach/resume, device loss and lane death get typed, reliably delivered
  contracts.

**Negative**

- The pacing model changes — the highest-severity risk, hence the CI budget gating acceptance.
- Adapter selection is permanent for the process; device loss blanks every window on the owner
  thread.
- A wedged raster thread leaks a native window.
- **macOS gains nothing yet**: it runs the inline lane (decision 1). The generation counter,
  two-axis freshness, reliable-slot contracts and per-owner-thread services still land there.
- The Android hot-reload plugin path requires the inline lane: its `Scene` would drop on the raster
  thread while the dylib may unload on the owner thread. Un-gating needs an unsafe audit.
- Web runs inline, with no pacing feedback (no visibility signal, no compositor tick).
- `flui-platform`'s window `Send`/`Sync` wrappers, `MacOSWindow` first, need an unsafe audit before
  the relay ships.

## Alternatives considered

- **One counter per side, reconciled on read.** Unsound — decision 4's false accept.
- **A request/ack handshake on resize.** Blocks the owner while the raster thread is parked in
  acquisition, at drag rates.
- **Keep `max_frames_in_flight` on an `Option<NonZeroU8>`, or ship both knobs.** A number with two
  reachable values out of 255, or two knobs for one bound.
- **Widen the mailbox into a queue.** Trades latest-frame-wins for staleness.
- **Share `GpuServices` process-wide.** Reintroduces a process-global GPU resource; revisit
  condition in decision 2.
- **Let the raster thread call `request_redraw` where "it works".** Rejected in decision 5; working on
  the backend CI executes says nothing about the ones it does not.
- **Detach a wedged thread and close its window anyway.** The surface would outlive its window.
- **Rely on `GpuResourceGeneration` gating alone after device loss.** Rejects a second lane's frames
  but never re-points it, so it starves; `ReplaceServices` is the missing half.
- **Carry the presented bit on the ack lane.** Its loss is correlated with the decision (newest
  dropped first).
- **A stall watchdog** — see below.

## Non-goals

- **A stall watchdog.** A "no completion across N produce attempts" detector fires on the
  legitimate case (a hidden window blocking in `get_current_texture()`, a first-use shader compile,
  a debugger pause) and is blind on the real one (a wedge with no ticker produces no attempts).
  Distinguishing wedged from slow needs a policy on how long an occluded present may block, and that
  policy has no owner. `LaneHealth`'s three states ship instead; the absence of `Stalled` is a
  decision.
- **Executable coverage for Win32, AppKit and Android hot-reload.** CI links and runs only winit and
  headless. Win32 and AppKit rest on audit plus named manual validation — wake delivery during a
  live-resize modal loop, `PostMessageW` against a destroyed HWND, `CFRunLoopSource` delivery in a
  common mode — and their registry entries stay `partial` regardless of CI colour.
- **Also out of scope:** parallel layout and parallel repaint boundaries (gated by ADR-0027's
  prerequisites); fine-grained `DamageRegion`; host-injected devices; process-level services,
  cross-thread recovery and adapter re-selection (decision 2); more than one raster thread per lane;
  pipeline depth above 2 (decision 6); composition callbacks on the raster thread — they fire on the
  owner thread through the post-frame lane, matching Flutter's UI-thread placement.

Widget-tree semantics, the three-tree model, lifecycle and the layout/paint/hit-test protocol are
unaffected: nothing here changes what a frame contains, only which thread produces its pixels and
what paces the thread that asks for one.
