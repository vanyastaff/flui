# Spec: Renderer owns its surface target (issue #1043)

- **Status:** Approved (standing autonomy; approach chosen after chief-architect draft + harsh-critic pass)
- **Slug:** `1043-renderer-surface-ownership`   ·   **Date:** `2026-09-14`   ·   **Owner:** chief-architect (engine) with systems-perf-lead (unsafe/platform)
- **Governing ADR:** `docs/adr/ADR-0063-renderer-owns-its-surface-target.md` (new) + amendment note on `docs/adr/ADR-0045-raster-lane.md` decision 1 and §7

## Problem
Carried from `intent.md` — a `#![forbid(unsafe_code)]` consumer of `flui-engine`'s public API can
write `let r = Renderer::new(&window).await?; drop(window); r.recover().await?;`; it type-checks,
and `recover()` then builds a wgpu surface from raw handle bytes whose pointee no longer exists. The
SAFETY argument that makes today's `unsafe` block sound lives in another crate's ownership
convention. The same saved-bytes recovery is already wrong on Android (fresh `ANativeWindow` after
resume), and `WindowsWindow::window_handle()`'s own comment records that it hands out a handle whose
HWND may already be destroyed.

## Goals / Non-goals
**Goals**
- G1 The borrowed-window escape does not compile; the retained owner demonstrably keeps the native
  target alive for the surface's whole life (intent: "What fixed looks like", sentence 1).
- G2 `recover()` cannot reach a destroyed/suspended native handle: it re-acquires from the live owner
  and fails with a typed, recoverable error when the owner reports `Unavailable` (sentence 2).
- G3 `RawHandles`, its `unsafe impl Send`, and the `create_surface_unsafe` block are deleted; the
  crate's SAFETY story describes what the code enforces (sentences 3–4).
- G4 The platform half of the contract: every `PlatformWindow` implementation reports
  `HandleError::Unavailable` once its native window is destroyed or suspended, and the two native
  desktop backends stop orphaning the renderer on close (the critic's verified finding: nothing
  clears the callback slots on Win32/AppKit, so the frame closure pins lane → renderer → surface →
  `Arc<window>` forever after `DestroyWindow`/`[NSWindow close]`).
- G5 The ownership protocol is testable without GPU FFI (a `SurfaceLease<S>` seam), and the record
  (ADR, `ARCHITECTURE.md`, `runtime-contract.toml`) is corrected — including what is *not* executed.

**Non-goals (explicitly out of scope — intent "Not this")**
- The ADR-0045 raster-lane / windowed-`GpuServices` migration (#559). The lease is shaped to move
  there unchanged; it is not moved here.
- Fixing AppKit's off-main-thread `recover()` panic (ADR-0045 known gap).
- Android drop-on-`Paused`/`TerminateWindow` wiring (market shape per `survey.md` §3.2): a follow-up
  issue filed with the citation; #1043 makes re-acquisition *correct*, not the lifecycle complete.
- Executing native handle destruction under Miri or on Win32/AppKit/Android hardware — those backends
  are compile-only in CI (`cross-typecheck`); every claim about them is labelled "clippy-clean, never
  executed" in the ADR.

## Approach
Owned target, retained; wgpu's safe surface path; recovery re-acquires from the owner; the protocol
lives in a GPU-free seam. Lineage (`survey.md` §2): wgpu's `Surface::_handle_source`, iced's
`Window` blanket trait, egui-wgpu's `set_window(Arc<Window>)`, vello's pass-through. The rejected
shapes are bevy's raw-bytes-plus-`unsafe` (what FLUI has today) and blade's borrowed-safe
`create_surface(&W)` (the defect, shipped by a peer).

### Key types (flui-engine)
```rust
// src/wgpu/window_target.rs (new)
/// What a windowed renderer needs from the thing it draws into: a handle source it can OWN
/// (`'static`), share with wgpu's `Surface` (`Send + Sync`, wgpu's `WindowHandle` bound on
/// native), and re-query on recovery. Blanket-implemented; effectively sealed.
pub trait WindowTarget: HasWindowHandle + HasDisplayHandle + Send + Sync + 'static {}
impl<T: ?Sized + HasWindowHandle + HasDisplayHandle + Send + Sync + 'static> WindowTarget for T {}

// src/wgpu/surface_lease.rs (new) — the protocol, generic over the surface so it runs under Miri
pub(crate) struct SurfaceLease<S> {
    /// Declared FIRST: dropped before `target`. (wgpu's own `Surface` also holds an `Arc` clone
    /// in `_handle_source`, declared last there — both orderings agree.)
    surface: S,
    target: Arc<dyn WindowTarget>,
}
impl<S> SurfaceLease<S> {
    /// Probe the owner (`window_handle()`, `display_handle()`) — mapping `HandleError` to
    /// `EngineError::SurfaceTargetUnavailable` — then build `S` from `Arc::clone(&target)`.
    /// The probe duplicates the call wgpu makes inside `create_surface`; it exists ONLY because
    /// `wgpu::CreateSurfaceError` hides the `HandleError` (only Display text survives), so
    /// "the owner says the window is gone/suspended" and "the driver refused" would otherwise be
    /// one undifferentiated `SurfaceCreation`. Document this at the probe or the next reader
    /// deletes it.
    pub(crate) fn acquire(target: Arc<dyn WindowTarget>, build: impl FnOnce(Arc<dyn WindowTarget>) -> EngineResult<S>) -> EngineResult<Self>;
    /// Recovery: re-probe the SAME owner, rebuild `S`. Never touches saved bytes — there are none.
    pub(crate) fn reacquire(&mut self, build: impl FnOnce(Arc<dyn WindowTarget>) -> EngineResult<S>) -> EngineResult<()>;
    pub(crate) fn target(&self) -> &Arc<dyn WindowTarget>;
}
```
`Renderer`:
- `pub async fn new(target: impl WindowTarget) -> EngineResult<Self>` — keeps its name and its
  `#[doc(hidden)]` supersession note verbatim (ADR-0045 deletes it later). Wraps once:
  `Arc::new(target)` (an `Arc<dyn PlatformWindow>` becomes `Arc<Arc<dyn PlatformWindow>>` — forced,
  because `Arc<dyn PlatformWindow>` cannot upcast to `Arc<dyn WindowTarget>` without
  `PlatformWindow: WindowTarget`, which would invert the L2→L4 layer edge; one extra pointer chase
  per surface creation; documented, not fixed).
- `build_windowed_gpu_stack(target: &Arc<dyn WindowTarget>, w, h)` calls
  `instance.create_surface(Arc::clone(target))` — wgpu's SAFE path via rwh's `Arc<H>` blankets and
  wgpu's `From<T: DisplayAndWindowHandle>`; `Surface<'static>` with no `unsafe`.
- The target lives ONLY in `GpuStackOrigin::OwnedWindowed { lease: SurfaceLease<wgpu::Surface<'static>> }`
  (variants `OwnedOffscreen`, `SharedServices` unchanged); today's two-fact split
  (`raw_handles.window.is_some()` + `gpu_stack_origin`) collapses to one enum.
- `recover()` matches the origin: `OwnedWindowed` → `lease.reacquire(..)` rebuilding the stack;
  `OwnedOffscreen` → device-only; `SharedServices` → `SharedServicesNotRecoverable`.
- Deleted: `RawHandles`, its `unsafe impl Send` and 78-line SAFETY comment, `raw_handles_field_pin`,
  the `create_surface_unsafe` block, the `compile_fail/raw_handle_field_breaks_send_derivation.rs`
  pin (its mechanism no longer exists — replaced by the escape pin below).
- `Renderer: Send` stays a derivation (`Arc<dyn WindowTarget>: Send + Sync` by bound;
  `Surface<'static>: Send + Sync` per wgpu, on wasm32 via the already-enabled
  `fragile-send-sync-non-atomic-wasm`). `Renderer: !Sync` today rides on
  `pre_present_hook: Option<Box<dyn FnMut() + Send>>` — accidental; add an explicit
  `_single_mutator: PhantomData<Cell<()>>` field with the runtime-contract's "single mutator, never
  shared" sentence so the pinned bound has a field of its own.
- `EngineError::SurfaceTargetUnavailable { source: HandleError }` (enum is `#[non_exhaustive]`);
  `recoverability()`: `Unavailable` → `Recoverable` (suspended/closing owner; the runners' backoff
  loop retries every `Err` regardless — verified `device_recovery.rs` never consults
  `recoverability()`; the classification is for `direct.rs`'s log line and future consumers),
  `NotSupported` → `Fatal`, `_ =>` `Fatal` (rwh's `HandleError` is `#[non_exhaustive]`; the wildcard
  is deliberate and commented).

### flui-platform obligations (the platform half)
- `PlatformWindow::window_handle`/`display_handle` doc gains a MUST: return
  `Err(HandleError::Unavailable)` once the native window is destroyed or suspended. winit's own
  SAFETY contract is the model ("never deallocated while the window is alive") — `WinitWindow`
  inherits it; `AndroidWindow` already conforms; `HeadlessWindow` already returns `Unavailable`.
- **Win32:** `HasWindowHandle for WindowsWindow` consults `hwnd_affinity::teardown_route` (identity
  probe, by-value OS queries, no deref, no `ForeignThread` verdict — verified) and returns
  `Unavailable` on `AlreadyGone`/`StaleHandle`. The `WM_DESTROY` arm calls `ctx.callbacks.clear()`
  after `dispatch_close()` — the HWND is still valid inside `WM_DESTROY` (until `WM_NCDESTROY`), so
  the swapchain/surface pinned by the frame closure is released BEFORE the native window is gone,
  and the pre-existing orphan (lane → renderer → surface → `Arc<WindowsWindow>`, never freed) ends.
  The `L1108-1120` paragraph is rewritten to say what is closed (re-acquisition, orphaning) and what
  stays open (a surface still alive during `DestroyWindow` when `close()` is called from inside a
  leased callback — the `CallbackLease` restore hazard from #919 — recorded, not claimed fixed).
- **AppKit:** `MacOSWindow::new` sends `setReleasedWhenClosed:NO` (winit does the same; without it
  `[NSWindow close]` releases the alloc/init +1 that `Drop` releases again — a pre-existing
  over-release, and a dangling `ns_window` for every later `msg_send!`). `handle_close`
  (`windowWillClose:`, fired while the window is still valid) sets `closed: AtomicBool` then calls
  `callbacks.clear()`; `window_handle()` returns `Unavailable` when `closed`.
- Both native backends: **clippy-clean under `cross-typecheck`, never executed here** — stated in
  those words in ADR-0063 and in the PR.

### flui-app / examples
- Call sites pass `Arc::clone(&window)` (flui-app ×4) / `Arc::clone(&window)` or the handle by value
  (examples ×4). `direct.rs` and `runner/web.rs` `recover()` arms log the new variant. The desktop
  runner's step-4 comment names the cycle window → callbacks → lane → renderer → `Arc<window>`
  (already present via the pre-present hook) and `complete_window_close`/`WM_DESTROY`/
  `windowWillClose` `clear()` as its sole breakers.
- `PresentationState`: no field change (it holds the window `Weak` and a `RenderingFlutterBinding`,
  not the `wgpu::Renderer` — the architect's draft misread this; corrected in `approaches.md`).

### Alternatives considered
| Option | Trade-off | Why not chosen |
|--------|-----------|----------------|
| A1 Borrowed `Renderer<'w>` | Encodes surface-outlives-target by lifetime; forbids `drop(window)` but not `window.close()`; incompatible with `thread_local! APP_RUNTIME` (`'static`), `spawn_local` on web, and any future threaded lane | The owner already holds the window in an `Arc`; a lifetime models what ownership models better. (Two citations in the first draft were wrong — `PresentationState` holds a `Weak`, `raster.rs:43` has no `'static` — the rejection stands on the TLS/`spawn_local` legs.) |
| A3 `unsafe fn from_raw_handles` + safe owned ctor | Re-introduces stored bytes and the `unsafe impl Send` | Zero consumers: every site owns an rwh-typed value; an FFI host implements `HasWindowHandle` on its owner type (softbuffer/glutin/wgpu convention). Contradicts ADR-0045's "deleted rather than narrowed". |
| ALT-1 (critic) Presentation-owned surface: `Renderer::new(instance, surface)`, `recover(|instance| ..)` closure, no engine-side trait | Escape impossible by absence of API; matches `ReplaceServices { surface }`; fixes Android TerminateWindow as a side effect | Requires each of 8 callers to co-create the wgpu `Instance` with the surface (they must share a wgpu-core `Global` — see memory `wgpu-instance-and-surface-must-be-created-together`) — that IS the windowed `GpuServices` constructor of ADR-0045 decision 2 (#559), gated on a pacing measurement and a still-Proposed ADR. A P0 fix must not couple to it. `SurfaceLease<S>` is generic so it moves there unchanged. |
| Approach 2 as first drafted (target in `Renderer`, no seam, "runner drop order" residual) | Smallest diff | RESHAPE NEEDED (critic): protocol untestable without GPU; residual described with two mechanisms that do not exist (recovery only fires on the device-lost flag; nothing drops the renderer on native close). |

## Public surface & semver impact
- **Breaking (allowed, `#[doc(hidden)]` fn, no external consumers):** `Renderer::new<W: ?Sized + HasWindowHandle + HasDisplayHandle>(&W)` → `Renderer::new(impl WindowTarget)`.
- **New pub:** `flui_engine::wgpu::WindowTarget` (re-exported at the crate root beside `Renderer`);
  `EngineError::SurfaceTargetUnavailable` (+ ctor `EngineError::surface_target_unavailable`).
- **Contract change (flui-platform):** `PlatformWindow::window_handle`/`display_handle` doc MUST.
- **Deleted:** nothing public (`RawHandles` was private).
- `docs/runtime-contract.toml`: the `flui_engine::wgpu::Renderer` entry's `thread_affinity` no longer
  cites an `unsafe impl Send`; `failure_semantics` names the new variant; new `forbidden_pattern`
  `create_surface_unsafe` with `allow_files = ["examples/wgpu_window.rs", "examples/painting_demo/src/lib.rs"]`
  (the two standalone demos that build their own wgpu stack; ratchets the engine deletion).

## Acceptance criteria
Traced to intent's "What fixed looks like" (F1 escape / F2 recover / F3 unsafe gone / F4 record).
- [ ] **AC1 (F1)** trybuild compile-fail with a `.stderr` snapshot: the issue's proof program
  (`Renderer::new(&window)` where `window: W` local, then `drop(window)`) fails with the `'static`
  error (E0597/E0310 named in the snapshot). A bare `compile_fail` doctest is NOT acceptable (passes
  on any error).
- [ ] **AC2 (F1)** GPU-free, runs under `just miri`'s scope or plain nextest: `SurfaceLease<FakeSurface>`
  with a fake `WindowTarget` — (a) a `Weak` to the target upgrades while the lease lives and fails
  after `drop(lease)` with no other holder; (b) a recording `Drop` proves order surface → target;
  (c) `acquire` polled once inside a future then dropped leaks nothing (strong count returns to 1).
- [ ] **AC3 (F2)** fake target returning `Unavailable` → `Renderer::new` fails with
  `SurfaceTargetUnavailable` BEFORE `wgpu::Instance::new` (assert via the fake's call log, GPU-free).
- [ ] **AC4 (F2)** gpu-test job: `recover()` on a renderer whose fake target toggles
  `Ok → Unavailable → Ok` returns `SurfaceTargetUnavailable` in the middle and succeeds after; the
  recovered surface is built from the fresh handle (fake returns a new handle value; assert the
  second `window_handle()` call happened).
- [ ] **AC5 (F3)** `rg -n "RawHandles|create_surface_unsafe|unsafe impl Send" crates/flui-engine/src`
  → 0 hits; `assert_impl_all!(Renderer: Send)` and `assert_not_impl_any!(Renderer: Sync)` still
  compile; `just runtime-conformance-check` green with the new `forbidden_pattern`.
- [ ] **AC6 (F2, platform)** unit tests: `WindowsWindow::window_handle()` returns `Unavailable` for a
  `TeardownRoute::AlreadyGone`/`StaleHandle` identity (route the pure function, no OS call — the
  probe is already factored as a pure `route_teardown`); `MacOSWindow` `closed` → `Unavailable`
  (compile-only backends: the tests compile under `cross-typecheck` and are labelled unexecuted).
- [ ] **AC7 (G4)** headless/winit: `real_loop_tests.rs` (xvfb) — a frame callback owning a
  renderer-shaped value that holds `Arc<dyn PlatformWindow>`; after `complete_window_close` the
  value's `Drop` has run (goes red if `clear()` stops breaking the cycle). Plus a second case:
  `owner.quit()` with the window open → the value's `Drop` runs before `Platform::run` returns
  (this is the critic's "quit skips per-window close leaks window+surface" hole — if it is red
  today, the fix is in scope: `finish_shutdown` drains per-window closes; if out of reach, file
  and label).
- [ ] **AC8 (G5)** `tracing` line `flui.gpu event="surface_released"` (target/level per
  `flui-engine/AGENTS.md`) from `SurfaceLease::drop`; `just live-smoke-wayland` asserts it appears
  before the window-close line on BOTH close routes (compositor + programmatic) — today that
  harness asserts exit status only and is blind to a leak-instead-of-crash regression.
- [ ] **AC9 (F4)** ADR-0063 written; ADR-0045 amendment note (decision 1: the blanket impl is deleted
  by #1043 before the lane; §7: rationale becomes "the `Arc` does not own the native lifetime on
  Win32/AppKit" with flutter#190599/#169585 cited as the unbounded-wait failure);
  `crates/flui-engine/ARCHITECTURE.md` unsafe inventory and mapping entry updated;
  `lib.rs`/`mod.rs` examples updated; `windows/window.rs` gap paragraph rewritten honestly.
- [ ] **AC10 (F4)** every claim about Win32/AppKit/Android in the PR body and ADR carries
  "clippy-clean under `cross-typecheck`, never executed"; follow-up issues filed: Android
  drop-on-`Paused`/`TerminateWindow` (survey §3.2), native manual validation (like #654).
- [ ] `just ci` green; `cross-typecheck` green for all three targets; `just wasm-check` green.

## Risks & open questions
- Win32 `clear()` inside `WM_DESTROY` while a callback is leased (close from inside a callback):
  the lease's `Drop` restores the closure after the clear — same hazard #919 solved on winit by
  owner-deferral. Mitigation in scope: `WindowCallbacks` gets a `cleared` latch that `CallbackLease`
  consults before restoring (verify `shared/handlers.rs`); if that is larger than it looks, record
  the residual and file it.
- AppKit `setReleasedWhenClosed:NO` changes when the `NSWindow` is deallocated (now: last wrapper
  `Drop`, as the code already assumes). Unexecuted here.
- Second `Arc` allocation per construction: negligible (construction-time only).
- The `quit()`-skips-close leak (AC7 case 2) may widen scope into `finish_shutdown`; bounded by
  labelling if it cannot be closed in this PR.

## Links
- `intent.md`, `approaches.md` (architect draft; A1 citations corrected here), `survey.md`
  (Flutter + market, user-requested), critic pass (this session), issue #1043, ADR-0045, #713, #919,
  #559, #654; memory: `wgpu-instance-and-surface-must-be-created-together`,
  `window-teardown-must-be-owner-deferred`, `shipped-seams-never-wired`.
