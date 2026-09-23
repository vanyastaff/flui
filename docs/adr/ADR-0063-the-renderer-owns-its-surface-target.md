# ADR-0063: The renderer owns its surface target; raw handles are never saved

- **Status:** Accepted
- **Date:** 2026-09-14
- **Amends:** ADR-0045 decision 1 and §7 (see the end of this record)

*A `wgpu::Surface<'static>` is sound only while the window behind it exists. The engine used to
prove that with another crate's convention; now it proves it with ownership, and recovery asks
the live owner instead of reading saved bytes.*

## Context

`Renderer::new(&W)` took a borrow, extracted `RawWindowHandle`/`RawDisplayHandle`, called
`Instance::create_surface_unsafe`, and stored the raw values in a `RawHandles` newtype with a
hand-written `unsafe impl Send`; `recover()` rebuilt the surface from those bytes. The SAFETY
comment relied on "flui-app's `App` owns the window for the renderer's whole life" — not a
precondition any safe caller had to uphold. Issue #1043: a `#![forbid(unsafe_code)]` crate
compiles `Renderer::new(&window).await?; drop(window); r.recover().await?`.

Two shipped defects sat behind the same assumption:

- **Android.** `MainEvent::Pause` does not touch the window; the `ANativeWindow` pointer is cleared
  only by `AppCmd::TermWindow`, applied after the `TerminateWindow` callback returns.
  `window_handle()` re-queries `native_window()` on every call — what holds a stale handle is the
  `wgpu::Surface` built once from it. The fix is releasing the surface at the right moment.
- **Win32.** The handle `window_handle()` hands out is tied to the `Arc<WindowsWindow>`, not to
  the HWND, which `close()` destroys synchronously.

The unsafe path was not load-bearing: wgpu's safe
`create_surface(SurfaceTarget::DisplayAndWindow(Box<dyn _>))` queries both handles itself and keeps
the boxed owner alive inside the `Surface`, dropped after all other fields.

**Market.** wgpu's examples, iced, egui-wgpu and vello all hand wgpu an owned `'static` target.
Bevy keeps raw bytes plus `unsafe impl Send` (FLUI's old shape) to cross a pipelined render
thread, paying with a convention-based SAFETY comment. blade-graphics (GPUI) ships the
borrowed-safe `create_surface(&W)` that #1043 names, sound only by GPUI's ownership discipline.

## Decision

1. **`Renderer::new(target: impl WindowTarget)`** with
   `pub trait WindowTarget: HasWindowHandle + HasDisplayHandle + Send + Sync + 'static {}`,
   blanket-implemented (`flui-engine/src/window_target.rs`). The renderer wraps the target once
   (`Arc<dyn WindowTarget>`) and hands a clone to wgpu's safe `create_surface`. The borrowed escape
   no longer type-checks; the owned form is sound because the surface keeps its owner alive. The
   constructor stays `#[doc(hidden)]` until ADR-0045's windowed `GpuServices` path replaces it.
2. **Recovery asks the owner.** The target lives only in the renderer's
   `lease: SurfaceLease<wgpu::Surface<'static>>`; `recover()` re-probes the retained owner and
   rebuilds. `RawHandles`, its `unsafe impl Send` and the `create_surface_unsafe` block are gone.
   `Renderer: Send` is derived with no manual impl; `Renderer: !Sync` is stated by its own
   `PhantomData<Cell<()>>` field rather than by accident of another field's type.
3. **Probe before creating, and type the answer.**
   `EngineError::SurfaceTargetUnavailable { source: HandleError }` — recoverable for `Unavailable`
   (owner suspended or closing; the runners' backoff retries), fatal otherwise. The probe duplicates
   wgpu's own call because `wgpu::CreateSurfaceError` exposes the `HandleError` only as text;
   without it "the window is gone" and "the driver refused" are one undifferentiated error.
4. **The protocol lives in a GPU-free seam.** `SurfaceLease<S>` (`flui-engine/src/surface_lease.rs`:
   surface declared before target; `probe`, `release() -> Released`,
   `replace_surface(Released, S)`, `surface()`, `has_surface()`) is generic over the surface, so
   retention, drop order, cancellation of a half-built `Renderer::new`, and the `Unavailable` path
   are unit tests with fake targets and surfaces. `recover()`'s windowed re-probe needs a real
   window and has no executed test. When ADR-0045's presentation-owned surface arrives, the lease
   moves there unchanged.
5. **Every `PlatformWindow` backend owes `Err(HandleError::Unavailable)` once its native window is
   destroyed or suspended, and native backends release the renderer on close.**
   - winit: inherits winit's contract — the `Arc<winit::Window>` the surface holds *is* the native
     lifetime.
   - Win32: `window_handle()` consults the `hwnd_affinity::teardown_route` identity probe
     (`AlreadyGone`/`StaleHandle` → `Unavailable`). `WM_DESTROY` clears the window's callback slots
     after `on_close`, while the HWND is still valid.
   - AppKit: `setReleasedWhenClosed:NO` (the +1 is released once, in `Drop`), a `closed` flag set
     from `windowWillClose:` before callbacks are cleared, `window_handle()` → `Unavailable` when
     closed.
   - `WindowCallbacks::clear()` latches: a `CallbackLease` returning after the clear drops its
     callback instead of restoring it. That closes the #919 frame-closure → lane → renderer →
     surface → window cycle structurally on every backend with a clear site — android, macos,
     windows, winit and headless all have one.
   - Android reports `Unavailable` only for a *terminated* window (`native_window()` is `Some`
     across an ordinary pause, `None` between `TerminateWindow` and the next `InitWindow`), so its
     release is driven by decision 6's signal, not by a probe. Its callback clear sits on `run`'s
     loop-exit path (take the platform's window, `WindowCallbacks::clear()`, then `invoke_quit()`),
     not in the `Destroy` arm: a `quit()` returns from `android_main` and the glue then finishes
     the activity, so a later `Destroy` is never read. On the `Destroy` route the surface is
     already gone. Nothing can re-register afterwards (`on_ready` is `FnOnce`; a recreated
     activity runs a new `android_main` against a new platform).
   - **Named gap: a panic that unwinds out of `AndroidPlatform::run`** skips the exit-path clear,
     stranding that activity's window, lane and renderer until process exit. A `Drop` guard is
     rejected: it would drop a configured `wgpu::Surface` during unwind (reaching
     `vkDeviceWaitIdle`/`vkDestroySurfaceKHR` on a device that may be the panic's cause), and a
     second panic aborts the process instead of letting the glue finish the activity. winit's
     clear is likewise reached only on returning routes. The structural fix is the raster lane
     owned by the realm and dropped by `teardown_platform_realm` (ADR-0045, #559).
   - Win32, AppKit and Android changes are type-checked by `cross-typecheck`, not executed on the
     development host.
6. **The renderer releases its surface on a signal and rebuilds unconditionally when a window
   returns.** `Renderer::release_surface()` and `Renderer::recreate_surface() -> EngineResult<()>`;
   a frame acquired while released sees `SurfaceAcquireOutcome::Released` (a skip, distinct from
   `Lost`, which would churn generations through a deliberately absent surface). `flui-platform`
   carries the signal as the per-window `on_surface_status_change(Box<dyn FnMut(bool) + Send>)`:
   `false` before the handle dies, `true` when one is available again. It defaults to a no-op, so a
   backend that never emits it conforms by absence (the surface is never released). `flui-app`
   answers in `runner/surface_lifecycle.rs`'s `ensure_surface`, returning
   `SurfaceLifecycleOutcome::{Released, Recreated, Failed(EngineError)}`; `Recreated` obliges the
   caller to mint a surface generation and mark a full repaint (`#[must_use]` catches a dropped
   value, not an ignored variant).
   - **Only the surface is rebuilt, never the GPU stack.** Width/height are kept (a resize can
     arrive while released); usage, format, present mode and alpha mode are re-derived from the new
     surface's capabilities, since `Surface::configure` panics on a stale one. If the format
     changed, the pipelines and offscreen pool are rebuilt — otherwise the mismatch shows only as a
     validation error and a blank window.
   - **The two directions are asymmetric on purpose.** `false` is idempotent (releasing twice is
     one release). `true` never consults what is held and always rebuilds, so a lost `false` cannot
     strand a surface built from a dead window for the rest of the process. The residual cost of a
     lost `false`: the old surface is dropped at the next `true`, after its `ANativeWindow` died. On
     Vulkan (the backend selected on Android) that is not a use-after-free — the surface holds a
     reference on the window — but it is a disconnect from a dead producer. The EGL path is
     unverified.
   - **`recreate_surface` releases before it creates** (#1185). `vkCreateAndroidSurfaceKHR` refuses
     a second surface for a live window, and wgpu-hal `expect`s that result, so create-first would
     abort the process on a `true` over a held surface (a missed `false`, or `InitWindow` before
     `Resume`). A failed create leaves the presentation released; the next `true` re-asks. A
     held-surface short-circuit is deliberately not added. `recover()` had the same create-over-held
     order on the ordinary device-loss path and releases first too.
   - **The release blocks on `vkDeviceWaitIdle`, accepted.** Dropping a configured surface
     unconfigures the swapchain, which waits for device idle. On Android this runs inside the
     `TerminateWindow` callback on the event-loop thread, while the Java UI thread is parked in
     `set_window` until that callback returns. The wait is unmeasured (no device on the host). It
     is the price of dropping while the handle is still valid; deferring is the
     `onSurfaceDestroyed` mistake below.
   - **The cost is per edge.** Android maps `false` to both `Pause` and `TerminateWindow`, `true` to
     both `Resume` and `InitWindow`. Pauses that never destroy the window (a dialog, multi-window
     deactivation) now pay a release and a full repaint. `TerminateWindow` alone would be enough for
     safety; `Pause` is where the framework learns the app is going away.
   - **Evidence is partial.** The lease, the `Released` disposition and `ensure_surface` are host
     unit tests; the Android arms that emit the signal are type-checked only; the renderer-side
     mechanics need a GPU. Release and rebuild emit their own trace events
     (`surface_released_by_owner`, `surface_recreated`), separate from the lease-drop line the
     desktop live-smoke harnesses assert at window close.

## Alternatives rejected

- **`Renderer<'w>` (borrowed lifetime).** Forbids `drop(window)` but not `window.close()`; the
  thread-local runtime, the web runner's `spawn_local` and any threaded lane need `'static`.
  Ownership already models this better.
- **`unsafe fn from_raw_handles` beside the safe constructor.** No consumer needs it; an FFI host
  implements `HasWindowHandle` on its own owner type (the softbuffer/glutin/wgpu convention). It
  would also reintroduce the stored bytes.
- **Presentation-owned surface now** (`Renderer::new(instance, surface)`). ADR-0045's end state,
  and the strongest alternative, but instance and surface must be created together (they share a
  wgpu-core `Global`), so every caller would own the instance — the windowed `GpuServices`
  constructor of #559, gated on a still-Proposed ADR. A soundness fix must not wait on it; the
  generic lease relocates unchanged.
- **Keep `RawHandles`, add an `Arc`, narrow the SAFETY comment.** The protocol would stay
  untestable without a GPU, and the residual argument rested on mechanisms that did not exist.

## Divergence from Flutter

Flutter (`Shell::OnPlatformViewDestroyed`) enforces "surface torn down before the native window
goes" by posting `Rasterizer::Teardown` to the raster thread and waiting on a latch with no
timeout; flutter/flutter#190599 and #169585 are that latch deadlocking behind a raster thread
wedged in `dequeueBuffer` — an ANR. ADR-0045 §7's bounded wait plus quarantine (leak the window and
thread rather than hang the owner) is the recorded improvement.

Flutter's Android embedding replaced the after-the-fact `onSurfaceDestroyed` with the
before-signal `onSurfaceCleanup` (#160933). Decisions 5 and 6 apply the same lesson: release while
the native object is still valid.

Flutter's Windows embedder has no device-loss recovery (#190383, #124194); FLUI's
`device_lost` → `recover()` → backoff loop is what this record makes sound.

## Consequences

**Positive**
- The renderer's production `unsafe` block and manual `Send` impl are gone; remaining SAFETY
  stories are ones the code enforces.
- Device-loss recovery after an Android pause/resume rebuilds against the live `ANativeWindow`, by
  construction of the shared path (not executed on a device).
- The frame-closure → renderer → surface → window cycle breaks at native close on every desktop
  backend and at loop exit on Android; a close requested from inside a leased callback cannot
  resurrect the frame closure.

**Negative**
- `Renderer::new` changed signature; call sites pass `Arc::clone(&window)`. One extra `Arc` per
  construction/recovery, and an `Arc<dyn PlatformWindow>` is wrapped in a second `Arc` (upcasting
  would need `PlatformWindow: WindowTarget`, inverting the platform → engine edge).
- **Still open:** on Win32 a surface is alive during `DestroyWindow` when close is requested from
  inside a leased callback (a logic error — the swapchain fails — not memory unsafety); the Android
  panic route above; a quit that skips per-window close is covered on winit by clearing every
  tracked window's callbacks at shutdown, pinned by a real-loop test.
- **AppKit trades a use-after-free for a bounded leak.** The native window maps are insert-only, so
  the wrapper's `Drop` (the only `NSWindow` release) never runs; the wrapper now keeps the window
  alive instead of dangling. Evicting the entry from the close path, owner-deferred rather than
  inside `windowWillClose:`, is #1147.
- Pauses that do not destroy the window cost a full repaint on Android.

## Amendments to ADR-0045

- **Decision 1.** The blanket `unsafe impl Send` is deleted here, before the raster lane. The
  in-`Renderer` retention of the target is transitional; the lease moves to the presentation when
  the windowed `GpuServices` constructor lands.
- **§7.** Quarantine's rationale no longer cites the `create_surface_unsafe` precondition; it reads
  "the `Arc` does not own the native lifetime on Win32/AppKit, and the reference's unbounded wait
  is an ANR in production". The decision itself is unchanged.
