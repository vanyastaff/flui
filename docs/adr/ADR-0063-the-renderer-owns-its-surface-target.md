# ADR-0063: The renderer owns its surface target; raw handles are never saved

*A `wgpu::Surface<'static>` is sound only while the window behind it exists.
The engine used to prove that with another crate's convention. Now it proves
it with ownership, and recovery asks the live owner instead of reading bytes.*

---

- **Status:** Accepted
- **Date:** 2026-09-14
- **Deciders:** @vanyastaff
- **Scope:** `flui-engine`'s `wgpu::Renderer` construction and recovery
  (`WindowTarget`, the private `SurfaceLease<S>`), `EngineError`, and the
  `PlatformWindow::window_handle`/`display_handle` contract every
  `flui-platform` backend owes. Amends ADR-0045 decision 1 and §7 (below).

---

## Context

`Renderer::new(&W)` took a borrow, extracted `RawWindowHandle` /
`RawDisplayHandle`, called `Instance::create_surface_unsafe(SurfaceTargetUnsafe::RawHandle { .. })`,
and kept the two raw values in a private `RawHandles` newtype with a
hand-written `unsafe impl Send` (a 78-line per-platform argument).
`recover()` rebuilt the surface from those saved bytes. The `unsafe` block's
SAFETY comment said the handles stay valid "because flui-app's `App` owns the
window for the `Renderer`'s whole life".

Issue #1043 showed the proof was not a precondition any safe caller had to
uphold: a `#![forbid(unsafe_code)]` crate depending only on `flui-engine`
compiles `let r = Renderer::new(&window).await?; drop(window); r.recover().await?;`
(`cargo check` exit 0, rustc 1.98.1). `#[doc(hidden)]` restricts nothing.

Re-reading the code before designing the fix turned the contract proof into
two shipped defects:

- **Android.** `AndroidWindow::window_handle()` answers
  `HandleError::Unavailable` between `Paused` and `Resumed`, and a *different*
  `ANativeWindow` after resume. Device-loss recovery
  (`runner/device_recovery.rs` → `recover()`) rebuilt the surface against the
  pointer captured at construction.
- **Win32.** `WindowsWindow::window_handle()`'s own SAFETY comment recorded
  that the handle it hands out is tied to the `Arc<WindowsWindow>`, not to
  the HWND, which `PlatformWindow::close()` destroys synchronously — "a real
  gap this comment does not close".

And the unsafe path was not load-bearing. Commit a6375b7c ("wgpu 29
multi-monitor fix") only made the display handle explicit; the raw-handle
target predates it, and wgpu 30.0.1's *safe*
`Instance::create_surface(SurfaceTarget::DisplayAndWindow(Box<dyn DisplayAndWindowHandle>))`
queries `window_handle()` and `display_handle()` itself and keeps the boxed
owner alive inside the `Surface` — the field `_handle_source`, declared last
with wgpu's own note that it "must be dropped *after* all other fields".

The market agrees on the shape (`.rust-studio/specs/1043-renderer-surface-ownership/survey.md`):
wgpu's examples, iced (`pub trait Window: HasWindowHandle + … + 'static {}`,
`create_surface(window: impl Window + Clone)`), egui-wgpu
(`set_window(Option<Arc<winit::Window>>)`), and vello all hand wgpu an
*owned* `'static` target. bevy keeps raw bytes plus `unsafe impl Send` — the
shape FLUI had — because it ships handles across a pipelined render thread and
pays with a convention-based SAFETY comment and an `unsafe fn get_handle()`.
blade-graphics (GPUI's backend) ships the borrowed-safe
`create_surface(&W) -> Surface` — the defect #1043 names — sound only by GPUI's
ownership discipline.

## Decision

1. **`Renderer::new(target: impl WindowTarget)`**, where
   `pub trait WindowTarget: HasWindowHandle + HasDisplayHandle + Send + Sync + 'static {}`
   is blanket-implemented (effectively sealed). The renderer wraps the target
   once (`Arc<dyn WindowTarget>`) and hands a clone to wgpu's safe
   `create_surface`. The borrowed escape no longer type-checks (`'static`);
   the owned form is sound because the surface itself keeps the owner alive.
   The name and the `#[doc(hidden)]` supersession note stay: ADR-0045
   decision 2 deletes this constructor when the windowed `GpuServices` path
   lands, and a rename would buy nothing before then.
2. **Recovery asks the owner.** The target lives only in
   `GpuStackOrigin::OwnedWindowed { lease: SurfaceLease<wgpu::Surface<'static>> }`;
   `recover()` re-probes `window_handle()`/`display_handle()` on that retained
   owner and rebuilds. There are no saved bytes to reuse. `RawHandles`, its
   `unsafe impl Send`, and the `create_surface_unsafe` block are deleted;
   `Renderer: Send` is a derivation with zero manual impls behind it, and
   `Renderer: !Sync` gets a field of its own (`PhantomData<Cell<()>>`) instead
   of riding on the pre-present hook's `Box<dyn FnMut + Send>` by accident.
3. **The engine probes before creating, and types the answer.**
   `EngineError::SurfaceTargetUnavailable { source: HandleError }` —
   `Recoverable` for `Unavailable` (the owner is suspended or closing; the
   runners' backoff retries), `Fatal` otherwise. The probe duplicates the
   call wgpu makes inside `create_surface` and exists only because
   `wgpu::CreateSurfaceError` exposes the `HandleError` as Display text and
   nothing else; without it "the owner says the window is gone" and "the
   driver refused" are one undifferentiated `SurfaceCreation`.
4. **The protocol lives in a GPU-free seam.** `SurfaceLease<S>` — surface
   declared before target, `acquire`/`reacquire`/`probe` — is generic over the
   surface so retention (a `Weak` to the target upgrades exactly while the
   lease lives), drop order (recording `Drop`s), cancellation (a future
   polled once and dropped leaks nothing), and the Unavailable path are unit
   tests with a fake target and a fake surface, not GPU runs; `Renderer::new`
   itself is pinned GPU-free (a fake target answering `Unavailable` fails
   the constructor before `wgpu::Instance::new`). `recover()`'s windowed
   re-probe has **no executed pin** — it needs a real window, and
   `flui-engine` has no windowing dev-dependency; it is verified by reading,
   and recorded here as such. When ADR-0045's presentation-owned surface
   arrives, the lease moves there unchanged.
5. **Every `PlatformWindow` backend owes `Err(HandleError::Unavailable)` once
   its native window is destroyed or suspended**, and the two native desktop
   backends release the renderer on close:
   - winit: inherits winit's own contract ("the window handle will never be
     deallocated while the window is alive") — the `Arc<winit::Window>` the
     surface holds *is* the native lifetime.
   - Win32: `window_handle()` consults the existing
     `hwnd_affinity::teardown_route` identity probe (`AlreadyGone` /
     `StaleHandle` → `Unavailable`; by-value OS queries, no deref, no
     foreign-thread refusal). The `WM_DESTROY` arm now `clear()`s the
     window's callback slots after `on_close`, while the HWND is still valid.
   - AppKit: `setReleasedWhenClosed:NO` at construction (the alloc/init +1 is
     released exactly once, in `Drop`; AppKit's default would release it
     again at `close`), a `closed` flag set from `windowWillClose:` before the
     callbacks are cleared, `window_handle()` → `Unavailable` when closed.
   - `WindowCallbacks::clear()` now latches; a `CallbackLease` returning after
     the clear drops its callback instead of restoring it (the #919 hazard,
     closed structurally on every backend).
   - Android already conforms (`native_window()` is `None` while paused).
   - **Win32, AppKit, and Android are clippy-clean under `cross-typecheck`
     and never executed here.** Those three sentences are verification
     claims of that strength and no more.

## Why the obvious alternatives were rejected

- **`Renderer<'w>` (borrowed lifetime).** Forbids `drop(window)` but not
  `window.close()`; and `thread_local! APP_RUNTIME`, the web runner's
  `spawn_local`, and any threaded lane need `'static`. The owner already
  holds the window in an `Arc` — a lifetime models what ownership already
  models better.
- **`unsafe fn from_raw_handles` beside a safe constructor.** Zero consumers:
  all eight call sites own an rwh-typed value. An FFI host that owns a bare
  native handle implements `HasWindowHandle` on its own owner type — the
  softbuffer/glutin/wgpu convention — and uses the safe constructor. It would
  also re-introduce the stored bytes ADR-0045 promised to delete.
- **Presentation-owned surface now (`Renderer::new(instance, surface)`,
  `recover(|instance| ..)`).** This is ADR-0045 decision 2's end state and
  the strongest alternative considered: the escape becomes impossible by
  absence of API. But the wgpu instance and surface must be created together
  (they share a wgpu-core `Global`; a foreign surface panics on
  `request_adapter`), so every caller would have to own the instance — that
  is the windowed `GpuServices` constructor of #559, gated on a pacing
  measurement and a still-Proposed ADR. A P0 soundness fix must not couple
  to it. The lease is the bridge: generic over `S`, it relocates without
  changing.
- **"Add `Arc`, keep `RawHandles`, narrow the SAFETY comment."** Rejected
  before any code was written: the protocol would stay untestable
  without a GPU, and the first draft's residual was described with two
  mechanisms that do not exist (recovery is reached only through the
  device-lost flag, never by a `SurfaceLost` after a close; nothing dropped
  the renderer on native close at all).

## Divergence from Flutter, and why it is an improvement

Flutter's engine (3.44.0, `shell/common/shell.cc`,
`Shell::OnPlatformViewDestroyed`) enforces "surface torn down before the
native window goes away" by posting `Rasterizer::Teardown` to the raster
thread and `latch.Wait()`ing **with no timeout** — "a synchronous operation
because certain platforms depend on … suspension of all activities that may
be interacting with the GPU in a synchronous fashion". flutter/flutter#190599
and #169585 are that latch in production: the platform thread deadlocked
behind a raster thread wedged in `Surface::dequeueBuffer` on the window being
hidden — an ANR. ADR-0045 §7's bounded wait plus quarantine (leak the window
and the thread rather than hang the owner) is the documented improvement,
and this record adds the reference accounting §7 lacked.

Flutter's Android embedding also learned that an after-the-fact signal is
too late: `SurfaceProducer.onSurfaceDestroyed` was replaced by the
before-signal `onSurfaceCleanup` because another thread could still touch
the dead surface (flutter/flutter#160933). Decision 5's `clear()` inside
`WM_DESTROY`/`windowWillClose:` is the same lesson — release the surface
while the native object is still valid.

Where FLUI is ahead: Flutter's Windows embedder has no device-loss detection
or recovery at all (flutter/flutter#190383, #124194 open since 2023; #190200
swallows `Present()` failures). FLUI's `device_lost` flag → `recover()` →
backoff loop is the mechanism #1043 makes sound.

## Consequences

**Positive**
- Net unsafe delta in `renderer.rs`: −1 `unsafe {}` block and −1 manual
  `Send` impl (the old file had exactly one of each; `git show
  53b04347:crates/flui-engine/src/wgpu/renderer.rs | rg -n 'unsafe impl|unsafe \{'`).
  Two test-only `borrow_raw` blocks arrive with the lease's fake target. The
  crate's remaining production SAFETY stories are ones the code enforces.
- Android device-loss recovery after a pause/resume cycle rebuilds against
  the live `ANativeWindow` — by construction of the shared code path, not
  by execution: the Android runner (`runner/android.rs`) and the demo are
  compiled by no gate (`cross-typecheck` builds `flui-platform` only).
- The frame-closure → lane → renderer → surface → `Arc<window>` cycle
  (present on every desktop backend through the pre-present hook) is broken
  at native close, not only on winit; a close requested from inside a leased
  callback can no longer resurrect the frame closure.

**Negative / trade-offs, stated**
- `Renderer::new` is a breaking signature change on a `#[doc(hidden)]`
  function; seven call sites pass `Arc::clone(&window)` instead of
  `window.as_ref()`, and the Android demo passes its handle by value. One extra `Arc` allocation per construction/recovery.
- An `Arc<dyn PlatformWindow>` is wrapped in a second `Arc` (it cannot upcast
  to `Arc<dyn WindowTarget>` without `PlatformWindow: WindowTarget`, which
  would invert the platform→engine layer edge). One extra pointer chase at
  surface creation; documented at the site.
- **Still open, named rather than absorbed:** a surface created earlier is
  alive during `DestroyWindow` when the Win32 close is requested from inside
  a leased callback (rwh's contract calls a deleted HWND a logic error — the
  swapchain fails — not memory unsafety); Android does not drop its surface
  on `Paused`/`TerminateWindow` (the market shape — filed as a follow-up
  with the survey's citations); a quit that skips per-window close leaks
  window + surface silently — closed for the quit route by clearing every
  still-tracked window's callback slots in the winit shutdown path, pinned by
  a real-loop test. Both live-smoke harnesses asserted only the exit code
  before this change (the Wayland variant's log filter did not even include
  `flui.gpu`), so the `surface_released` trace line the lease emits on drop is
  now asserted in both, on both close routes, before the loop's own quit
  line — the one executed witness that the surface was released rather than
  orphaned. The X11 variant ran here; the Wayland variant runs in CI, where
  `weston` is installed.
- The native backends' changes are compile-only verified (see decision 5).
- **AppKit trades a use-after-free for a bounded leak, deliberately.** Both
  native backends' window maps are only ever inserted into (`rg -n
  '\.remove\(|\.retain\(' crates/flui-platform/src/platforms/{windows,macos}/platform.rs`
  → 0 hits), so the wrapper's `Drop` — the only place `windows_map.remove`
  and the `NSWindow` `release` live — never runs. Before this record,
  AppKit's default `releasedWhenClosed` freed the `NSWindow` at `close`
  while the leaked wrapper kept a dangling pointer to it; now the wrapper
  keeps the window (and its view hierarchy) alive instead. Evicting the map
  entry from the close path is #1147, and it must be owner-deferred rather
  than done inside `windowWillClose:` — releasing our `+1` from within
  AppKit's own `close` dispatch could deallocate the object mid-method,
  which is why winit ties the release to its `Window`'s drop, not to the
  delegate callback.

## Amendments to ADR-0045

- **Decision 1.** "A blanket `unsafe impl Send` is deleted rather than
  narrowed" happens here, before the lane, not as a consequence of moving
  surface creation to the owner thread. The in-`Renderer` retention of the
  target is transitional; the lease relocates to the presentation when
  decision 2's windowed `GpuServices` constructor lands.
- **§7.** The rationale for quarantine no longer reads "the
  `create_surface_unsafe` precondition"; it reads "the `Arc` does not own
  the native lifetime on Win32/AppKit, and the reference's unbounded form of
  this wait is an ANR in production (flutter/flutter#190599, #169585)". The
  decision itself is unchanged.
