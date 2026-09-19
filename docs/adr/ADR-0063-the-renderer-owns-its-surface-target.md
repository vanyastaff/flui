# ADR-0063: The renderer owns its surface target; raw handles are never saved

*A `wgpu::Surface<'static>` is sound only while the window behind it exists.
The engine used to prove that with another crate's convention. Now it proves
it with ownership, and recovery asks the live owner instead of reading bytes.*

---

- **Status:** Accepted
- **Date:** 2026-09-14
- **Deciders:** @vanyastaff
- **Scope:** `flui-engine`'s `wgpu::Renderer` construction, recovery, and
  surface release/rebuild (`WindowTarget`, the private `SurfaceLease<S>`),
  `EngineError`, and the `PlatformWindow` contracts every `flui-platform`
  backend owes — `window_handle`/`display_handle`, and the
  `on_surface_status_change` availability signal. Amends ADR-0045 decision 1
  and §7 (below).

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

- **Android.** `AndroidWindow::window_handle()` was recorded as answering
  `HandleError::Unavailable` *between `Paused` and `Resumed`*, and as being
  rebuilt by device-loss recovery "against the pointer captured at
  construction". Both halves are wrong, and the correction is decision 6's
  premise. `MainEvent::Pause` does not touch the window at all: the pointer is
  cleared only by `AppCmd::TermWindow`, which `android-activity` 0.6.1 applies
  *after* the `MainEvent::TerminateWindow` callback returns, so an ordinary
  pause leaves `native_window()` answering `Some`. And nothing captures a
  pointer in FLUI: `window_handle()` re-queries `native_window()` and rebuilds
  the raw handle on every call. What holds an old handle is the
  `wgpu::Surface` created once from whichever handle it was handed — which is
  why releasing it at the right moment, rather than rebuilding later, is the
  fix.
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
     the clear drops its callback instead of restoring it. That is the #919
     hazard's structural close **on every backend that has a clear site** —
     `platforms/{android,macos,windows,winit,headless}` all do, and the census
     is `rg -n 'callbacks(\(\))?\.clear\(\)' crates/flui-platform/src/platforms`
     — the optional `()` matters, because both forms are in use: winit's
     `complete_window_close` and `release_open_window_callbacks` and the
     Android exit path reach the slots through the `callbacks()` accessor,
     while macOS, Windows, headless and `WinitWindow::drop` call
     `callbacks.clear()` on the field, so a regex that requires the
     parentheses reports only the accessor sites. Run today it prints hits
     under `android/`, `headless/`, `macos/`, `windows/` and `winit/`, so the
     cycle is closed on all five backends.

     > *Amended 2026-09-16 (#1187):* Android joined the census, with its site
     > on the loop's exit path rather than in the `MainEvent::Destroy` arm —
     > the bullet below has the decision. Before that this bullet listed four
     > backends and named Android as the exception, a boundary of this record
     > rather than a gap in it.
   - Android reports `Unavailable` for a *terminated* window, and only then:
     `native_window()` is `Some` across an ordinary pause and `None` between
     `MainEvent::TerminateWindow` and the next `InitWindow`. A backend that
     only ever released on a pause would therefore still present through a
     surface whose handle the activity destroyed, which is why the release is
     driven by decision 6's signal rather than by a `window_handle()` probe.
   - **Android's registration cycle is closed on the loop's exit path, not in
     the `Destroy` arm.** The site is `platforms/android/mod.rs`'s `run`,
     between the loop's close and `invoke_quit()`: it `take()`s the platform's
     `window` field and calls `WindowCallbacks::clear()` on the taken window,
     which drops the `on_request_frame` (`runner/android.rs`) and
     `on_surface_status_change` closures (the only owners of the raster lane),
     and with them the renderer and the `Arc<AndroidWindow>` its surface
     lease holds. The exit path rather than the arm, because `Destroy` is one
     of three returning routes and not the one an application chooses: a
     `quit()` breaks the loop and returns from `android_main`, and
     `android-activity` 0.6.1's `rust_glue_entry` follows that return with
     `ANativeActivity_finish`, so the `Destroy` the JVM thread writes
     afterwards lands in a pipe this loop no longer reads. This mirrors the
     hole winit closed with `release_open_window_callbacks`, whose own doc
     names `owner.quit()` and a failed `on_ready` as the two routes
     `complete_window_close` never runs for. On the `Destroy` route the
     surface is already gone: AOSP's `NativeActivity.onDestroy` destroys it
     before it unloads the native code, and `set_window(None)` parks the JVM
     thread until `TermWindow` has been applied, so the
     `MainEvent::TerminateWindow` callback has fully returned before `Destroy`
     is written. The Rust half of that pair is read from `android-activity`
     0.6.1's source; the AOSP half is read from `NativeActivity.java` on
     `refs/heads/main` rather than a pinned tag, so it is the platform's shape
     as of this record, not a frozen contract. Nothing can re-register on the
     cleared set afterwards:
     `on_ready` is `FnOnce`, and `ANativeActivity_onCreate` spawns one thread
     with a fresh `AndroidApp` per activity instance, so a recreated activity
     runs a new `android_main` against a new platform and a new window.

     > *Amended 2026-09-16 (#1187):* this bullet read "**The one registration
     > cycle that is still closed is Android's.**", and ended "Filed as a
     > follow-up." The site landed as the exit-path release described above,
     > under three returning routes plus the one exception below.
   - **The one route not covered is named: a panic that unwinds out of `run`.**
     `rust_glue_entry` wraps the call in `catch_unwind` and still finishes the
     activity, so a panic anywhere inside the loop (a frame closure, a widget
     build, an `expect("BUG: ..")`) unwinds past the exit region and leaves
     that activity's window, lane and renderer stranded until the process
     ends. A `Drop` guard armed before the loop would cover it and is
     rejected: it would run the clear during unwind, dropping a configured
     `wgpu::Surface`, which reaches `vkDeviceWaitIdle` and
     `vkDestroySurfaceKHR` on a device that may itself be the panic's cause,
     and a second panic there aborts the process instead of letting the glue's
     `catch_unwind` finish the activity; `runner/host.rs`'s `OwnerHostClearGuard`
     keeps its own `Drop` to a single `take()` for the same reason, and winit's
     clear is reached only on a returning route too: `WinitApp::finish_shutdown`
     runs from `exiting()` (`platforms/winit/platform.rs`, winit's own
     loop-exit callback, which the `&ActiveEventLoop` it takes proves is inside
     `run_app`), from `request_exit` (a `quit()` routed through the user-event
     handler), and once more from the post-`run_app` tail — and a panic
     unwinding out of a callback reaches none of them, so
     `release_open_window_callbacks` is skipped there as well. Whether
     `wgpu-hal`'s Vulkan release path can itself
     panic on `VK_ERROR_DEVICE_LOST` is **unverified**; the decision does not
     depend on it, because the abort hazard is the second panic, whatever
     raises it.

     > *Amended 2026-09-16 (#1187):* the coverage is "three returning routes,
     > plus one named exception", never "every route".

   - **Evidence.** A capture-release test on `WindowCallbacks` in
     `shared/handlers.rs` (both cycle-closing slots are dropped by
     `clear()`, probed through `Weak`, so a `clear_now` that took every slot
     and `mem::forget` the tuple fails on a named assertion); a function-scoped
     source guard in `crates/flui-platform/tests/` that locates `run`'s exit
     region and refuses an inverted one; compilation of the site by
     `cross-typecheck`'s Android line under `-D warnings` and by the NDK-free
     `flui-app` check. Nothing on this host executes the site, and no test
     here claims it does.
   - **The site is interim under the shape that retires it.** Once the raster
     lane is owned by the realm's slot and dropped by `teardown_platform_realm`
     on every returning route, with the window's closures holding only a
     handle, the platform no longer carries the cycle at all and the panic
     route closes structurally rather than by text. That is the direction
     ADR-0045 and issue #559 already point at, and it is out of scope here.
   - **Win32, AppKit, and Android are clippy-clean under `cross-typecheck`
     and never executed here.** Those three sentences are verification
     claims of that strength and no more.
6. **The renderer releases its surface on a signal, and rebuilds it
   unconditionally when a window returns.** `SurfaceLease::surface` becomes
   `Option<S>` (`release()`/`surface()`/`has_surface()`), `Renderer` gains
   `release_surface()` and `recreate_surface() -> EngineResult<()>`, and
   `SurfaceAcquireOutcome::Released` is the disposition a frame sees while
   released — distinct from `SurfaceLost`, which would churn a generation
   through a surface that is deliberately gone. `flui-platform` carries the
   signal as the per-window `on_surface_status_change(Box<dyn FnMut(bool) +
   Send>)` callback: `false` before the handle dies, `true` when one is
   available again. A backend that never emits it conforms by absence — the
   surface is never released, which is the pre-#1146 behavior — and the method
   defaults to a no-op, so all nine implementors compile unchanged. `flui-app`
   owns the response in `runner/surface_lifecycle.rs`'s `ensure_surface`, which
   returns `SurfaceLifecycleOutcome::{Released, Recreated, Failed(EngineError)}`;
   a `Recreated` obliges the caller to mint a surface generation on its raster
   lane and mark a full repaint. `#[must_use]` on the outcome buys a warning at
   the call site for a value that is dropped without being read, which is what
   turns a forgotten `Recreated` into a compile-time diagnostic instead of a
   silent omission; it buys no more than that, since `let _ = ...` silences it,
   and it says nothing about the obligation itself being discharged: a caller
   that reads the variant and then ignores it still compiles clean.
   - **Only the surface is rebuilt, never the stack**, and the target is the
     retained `Arc<dyn WindowTarget>`: both of its methods take `&self`, so it
     answers with whatever handle is current at the moment it is asked. The
     width/height are kept rather than re-derived (a resize can arrive while
     released); `usage`, `format`, `present_mode` and `alpha_mode` are
     re-derived from the new surface's capabilities, because
     `Surface::configure` panics on a stale one. `format` is the one of those
     four with consumers inside the renderer: the pipelines and the offscreen
     pool bake the format they are constructed with, so `recreate_surface`
     rebuilds both when the freshly derived format differs from the one they
     hold. Without that, the render attachment and the pipelines would
     disagree on every frame, and the mismatch surfaces only as a logged
     validation error on the uncaptured-error handler, which is a blank window
     rather than a failure. `release()`/`resize()`
     therefore keep the authoritative size while released, and
     `reconfigure_surface` is a documented no-op in that state.
   - **The two directions are asymmetric on purpose.** `false` asks for a
     post-state that can already hold, so releasing twice is one release;
     `true` asks about the present and never consults what is held, so it
     rebuilds even when a surface exists and never short-circuits. That is
     what removes the "a missed release strands a surface built from a dead
     window for the rest of the process's life" failure mode: the next `true`
     re-asks instead of being mistaken for a redundant one, so correctness
     does not depend on having received every `false` — which matters because
     a `false` can be lost (a registration in a slot nothing dispatches, an
     event order the arms do not map). What that statelessness covers is the
     stranded-surface failure mode only. A lost `false` still leaves a cost
     behind: the configured `wgpu::Surface` built from the dying handle is then
     dropped at the *next* `true` instead, inside `recreate_surface`'s
     `replace_surface`: after the `ANativeWindow` behind it is gone, which is
     the ordering this decision exists to avoid. Nothing on this side
     dereferences the handle along that path (the surface is dropped, not
     presented or reconfigured). On the Vulkan backend, which is the one this
     renderer selects on Android, the late drop is not a use-after-free
     either: the specification for `vkCreateAndroidSurfaceKHR` says a
     successful create "increments the `ANativeWindow`'s reference count, and
     `vkDestroySurfaceKHR` will decrement it", so the surface keeps the window
     it was built from alive for as long as the surface exists, and the late
     `vkDestroySurfaceKHR` decrements a count on memory the surface itself
     held. What the lost `false` leaves on Vulkan is a disconnect from a dead
     producer — a logic error of the class this record already names for
     Win32, not freed memory. The EGL path (`wgpu-hal`'s `gles` backend, not
     selected on Android here) is **unverified**: nobody has read whether
     `eglDestroySurface` on a window the app no longer holds dereferences it.
     That residual is what makes a lost `false` a degraded path rather than a
     sound one.
   - **The release's unbounded wait is `vkDeviceWaitIdle`, and it is
     accepted.** Dropping a *configured* `wgpu::Surface` runs `wgpu-core`'s
     `Drop for Surface` into `unconfigure`, which reaches `wgpu-hal`'s
     `Swapchain::release_resources` and calls `vkDeviceWaitIdle` before
     destroying anything — wgpu-hal's own comment: "there is no way to
     portably wait until the presentation work is done, we are forced to wait
     until the device is idle." Two threads are involved, and the wait sits on
     the one that did not ask for the transition. `set_window` runs on the
     Java/Android UI thread: it writes `AppCmd::TermWindow`, then parks in a
     timeout-free `cond.wait` until that command has been applied. The
     release, and with it `vkDeviceWaitIdle`, runs inside the
     `TerminateWindow` callback that `pre_exec_cmd`/`post_exec_cmd` bracket, on
     the thread executing the event loop (`poll_events`), the app's own main
     thread. So the Java UI thread stays blocked until that callback returns,
     which puts an unbounded device-idle wait on the Java UI thread's blocking
     path, bounded only by the driver finishing its presentation work.
     **Unverified:** neither wait's duration is measured here, there being no
     Android device on this host and the live-smoke harnesses exercising the
     desktop backends. It is the price of dropping while the
     handle is still valid, which is the whole point: the alternative —
     deferring to a later event — is the `onSurfaceCleanup`-vs-
     `onSurfaceDestroyed` lesson above. What the release can avoid, it does:
     it is a synchronous, surface-only act with no lane handoff of its own, no
     probe and no submit. What it cannot avoid is that its caller holds the
     raster lane's guard across it, deliberately, because the drop has to
     complete before the callback that asked for it returns.
   - **The cost is per edge, not per defect, and a second `true` on the same
     window re-creates cleanly.** Android maps `false` to both
     `Pause` and `TerminateWindow` and `true` to both `Resume` and
     `InitWindow`, and the seam attempts a recreation on every `true` (the
     asymmetry in the bullet above), so the count is per signal rather than
     per activity cycle. What a second `true` costs depends on whether a
     surface is still held. When it is not (the ordinary cycle, where the
     `false` between them released it), the second `true` rebuilds. When it is
     — a `true` arriving over a surface still bound to the *same*
     `ANativeWindow` — `recreate_surface` releases that surface before
     creating the replacement (see that method's doc and the amendment
     below), so the Vulkan rule that "only one `VkSurfaceKHR` can exist at a
     time for a given window" is never asked to admit a second one. It
     releases and re-creates rather than being refused: a refused create
     would be a panic rather than a `warn` on this stack, because
     `wgpu-hal` 30.0.1's `create_surface_android`
     (`src/vulkan/instance.rs`) `expect`s the `vkCreateAndroidSurfaceKHR`
     result ("AndroidSurface failed"). Two routes reach a second `true` over
     a held surface, and neither is the ordinary cycle, where every `true`
     follows a `false` that released: a `false` missed on a window that
     survived, and two `true`s with no `false` between them, which is the
     `InitWindow`-before-`Resume` ordering. It is the price of recreating
     without consulting what is held; a held-surface short-circuit is
     deliberately not added, because it would reintroduce the state the
     asymmetry bullet exists to avoid. On the
     cold-launch ordering (`Resume` first) the first `true` finds no window
     and its probe reports `SurfaceTargetUnavailable`, so only the
     `InitWindow` rebuild lands. Whether the other ordering, `InitWindow`
     before `Resume`, occurs at all is **unverified**: `android-activity`
     0.6.1's glue emits `Resume` before `InitWindow` on every path traced
     here, so the second route may describe an ordering that never happens.
     The `false` half is
     cheaper by the same asymmetry: the second release reaches a lease that
     already holds nothing and is a no-op. Both directions cover
     the pauses where the window is never destroyed (a dialog over the
     activity, a multi-window deactivation); in those cases
     the surface's contents were still valid and only incremental damage was
     needed, so the full repaint is a cost this decision adds. Keying on
     `Pause` alone would not be enough, and keying on `TerminateWindow` alone
     would be too late in the other direction: the pair is required because
     `TerminateWindow` is where the handle actually dies and `Pause` is where
     the framework is told the app is going away. **This closes the "Android
     does not drop its surface on `Paused`/`TerminateWindow`" follow-up this
     record used to carry.**
   - **The executed evidence is partial, and says so.** The lease, the
     `Released` disposition and `ensure_surface` are host unit tests; the
     Android arms that emit the signal are **type-checked by
     `just cross-typecheck` and executed by nothing**, and the `Renderer`-side
     mechanics need a GPU. Both live-smoke harnesses run the desktop demo,
     where no backend emits the signal, so the lease-drop line they assert
     still fires exactly once at window close; the explicit release and the
     rebuild carry their own distinctly named events
     (`surface_released_by_owner`, `surface_recreated`) rather than reusing
     the drop line, so the harness oracle keeps its meaning instead of being
     re-pointed at a path it does not exercise.

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
  53b04347:crates/flui-engine/src/renderer.rs | rg -n 'unsafe impl|unsafe \{'`).
  Two test-only `borrow_raw` blocks arrive with the lease's fake target. The
  crate's remaining production SAFETY stories are ones the code enforces.
- Android device-loss recovery after a pause/resume cycle rebuilds against
  the live `ANativeWindow` — by construction of the shared code path, not
  by execution: the Android runner (`runner/android.rs`) and the demo are
  compiled by no gate (`cross-typecheck` builds `flui-platform` only).
- The frame-closure → lane → renderer → surface → `Arc<window>` cycle
  (present on every desktop backend through the pre-present hook) is broken
  at native close, not only on winit, and on Android at loop exit; a close
  requested from inside a leased callback can no longer resurrect the frame
  closure.

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
  swapchain fails — not memory unsafety); a panic that unwinds out of
  `AndroidPlatform::run` skips its exit-path clear, so that activity's window,
  lane and renderer stay stranded for the process's life; left unguarded on
  the double-panic argument in decision 5 rather than absorbed; a quit that
  skips per-window close leaks
  window + surface silently — closed for the quit route by clearing every
  still-tracked window's callback slots in the winit shutdown path, pinned by
  a real-loop test. Both live-smoke harnesses asserted only the exit code
  before this change (the Wayland variant's log filter did not even include
  `flui.gpu`), so the `surface_released` trace line the lease emits on drop is
  now asserted in both, on both close routes, before the loop's own quit
  line — the one executed witness that the surface was released rather than
  orphaned. The X11 variant ran here; the Wayland variant runs in CI, where
  `weston` is installed.
  > *Amended 2026-09-16 (#1187):* this bullet dropped its Android clause,
  > which read "the Android backend still carries no `callbacks().clear()` site,
  > so its registration cycle stays closed" — the site landed, on the loop's
  > exit path (decision 5). What replaces it is the route that site cannot
  > cover: a panic unwinding out of `AndroidPlatform::run`. The Win32 and
  > quit-route clauses are unchanged.
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

## Amendment (2026-09-17): `recreate_surface` releases before it creates

Decision 6's bullet "The cost is per edge" originally described a second `true`
over a surface still bound to the same live `ANativeWindow` as **refused** by
the platform, with the held surface staying in place. The body text above now
describes the corrected behaviour; this records what changed and why.

**The order changed: `recreate_surface` releases the held surface before
calling `create_surface`.** It used to create first and commit last, which on
the one platform that emits this signal is a process abort rather than a
`Failed` outcome: `vkCreateAndroidSurfaceKHR` refuses a second surface for a
live `ANativeWindow`, and `wgpu-hal` 30.0.1's `create_surface_android`
`expect`s that result. The refusal was already documented as outside the
ordinary cycle (a missed `false`, or two `true`s with no `false` between
them), but "outside the ordinary cycle" is exactly the class this method must
stay safe for: it is stateless *because* a missed signal is undetectable from
here, so an unconditionally re-askable method may not contain an order that
aborts when the re-ask is not redundant.

**What the old order bought, and what is given up.** Build-first kept a
still-valid surface alive when the create failed. That is worth less than it
looks here: the old surface's window is either the same one — so the create is
refused, and keeping the old surface is the only alternative — or already
dead, in which case the surface is useless. On a create failure the
presentation is left released, which decision 6's own `Failed` arm already
documented, and the next `true` re-asks. The cost is one unconditional
surface release per recreation, whose `vkDeviceWaitIdle` price the decision-6
bullet on release already accounts for.

**What did not change.** The method stays stateless: it still never consults
whether a surface is held, and no held-surface short-circuit was added — that
would reintroduce the stranded-surface failure mode the asymmetry bullet
exists to avoid. `release_surface` is untouched. `recreate_surface`'s
"build FIRST, commit LAST" comment now applies only to
`configure` (which still precedes `replace_surface`), not to
`create_surface`.

**Evidence.** The executed pin is the drop-order pair in
`wgpu/surface_lease.rs` (`drop_order_is_surface_before_target`,
`release_drops_the_surface_and_keeps_the_target`), which covers the lease's
half of the ordering; the `Renderer`-side call order needs a GPU and is
compile-verified only, as decision 6's "The executed evidence is partial"
bullet already states for this whole path. Filed and closed as issue #1185.

**`recover()` carried the same defect on a reachable path, and is fixed the
same way.** `recover` rebuilds the windowed stack through the same
`build_windowed_gpu_stack`, whose `create_surface` ran while the lease still
held the old surface — but unlike `recreate_surface`, nothing released it
first: a lost device does not release its surface, and the device-loss flag is
the only precondition. So the abort this amendment removes from
`recreate_surface` was reachable through the ordinary device-recovery loop
(`flui-app`'s `render_frame_with_device_recovery` → `attempt_device_recovery`
→ `Renderer::recover`), not only through the missed-signal route decision 6
books. `recover` now releases the held surface before the rebuild, under the
caller's exclusive `&mut` borrow, so no frame is in flight. A failed rebuild
leaves the presentation released and the next attempt re-asks — the same
post-state `recreate_surface` documents. This instance was found while
verifying the `recreate_surface` change, not reported; it is recorded here
rather than silently folded in, because it makes the defect class
*reachable* rather than latent.
