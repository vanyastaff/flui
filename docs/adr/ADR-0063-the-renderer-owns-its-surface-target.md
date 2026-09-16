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
     `platforms/{macos,windows,winit,headless}` all do, and the census is
     `rg -n 'callbacks(\(\))?\.clear\(\)' crates/flui-platform/src/platforms`
     — the optional `()` matters, because winit reaches the slots through the
     `callbacks()` accessor while macOS, Windows and headless call
     `callbacks.clear()` on the field, and a regex that requires the
     parentheses reports only the winit files. Run today it prints hits under
     `headless/`, `macos/`, `windows/` and `winit/` and none under `android/`.
     Android has none, so the registration cycle stays closed there; that gap
     is a boundary of this record rather than a claim it closes (see the
     Android bullet below and decision 6).
   - Android reports `Unavailable` for a *terminated* window, and only then:
     `native_window()` is `Some` across an ordinary pause and `None` between
     `MainEvent::TerminateWindow` and the next `InitWindow`. A backend that
     only ever released on a pause would therefore still present through a
     surface whose handle the activity destroyed, which is why the release is
     driven by decision 6's signal rather than by a `window_handle()` probe.
   - **The one registration cycle that is still closed is Android's.** It
     registers `on_request_frame` (`runner/android.rs`) and nothing clears
     those slots: window → callbacks → frame callback → raster lane →
     renderer → surface → `Arc<Window>`, exactly the cycle the four clear-site
     comments describe. Adding the site is deliberately not part of this
     change — it changes behavior on a path no gate here executes, it
     interacts with activity recreation, and `MainEvent::Destroy` (the
     candidate site) sits one line away from the callback registration
     decision 6 adds, which `clear()` would silently break for the rest of the
     window's life. Filed as a follow-up.
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
     window is refused rather than paid twice.** Android maps `false` to both
     `Pause` and `TerminateWindow` and `true` to both `Resume` and
     `InitWindow`, and the seam attempts a recreation on every `true` (the
     asymmetry in the bullet above), so the count is per signal rather than
     per activity cycle. What a second `true` costs depends on whether a
     surface is still held. When it is not (the ordinary cycle, where the
     `false` between them released it), the second `true` rebuilds. When it is
     — a `true` arriving over a surface still bound to the *same*
     `ANativeWindow` — the platform refuses the rebuild instead of running a
     second create/configure pair: the Vulkan specification for
     `vkCreateAndroidSurfaceKHR` states that "only one `VkSurfaceKHR` can
     exist at a time for a given window" and returns
     `VK_ERROR_NATIVE_WINDOW_IN_USE_KHR` for the second, and
     `recreate_surface` creates before it commits, so the held surface is
     still alive at that moment. The old surface stays in place and stays
     valid; no second mint and no second repaint occur, because a refused
     create never reaches the mint. What the refusal costs is decided by the
     `wgpu-hal` in the lockfile rather than by this design: the seam's
     `Failed` arm would log it once at `warn` as a `SurfaceCreation` error,
     but `wgpu-hal` 30.0.1's `create_surface_android`
     (`src/vulkan/instance.rs`) `expect`s the `vkCreateAndroidSurfaceKHR`
     result ("AndroidSurface failed"), so under that version the refusal is a
     panic on the callback thread, not the `warn`. Two routes reach it, and
     neither is the ordinary cycle, where every `true` follows a `false` that
     released: a `false` missed on a window that survived, and two `true`s
     with no `false` between them, which is the `InitWindow`-before-`Resume`
     ordering. It is the price of recreating without consulting what is
     held; a held-surface short-circuit is deliberately not added, because it
     would reintroduce the state the asymmetry bullet exists to avoid. On the
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
  swapchain fails — not memory unsafety); the Android backend still carries no
  `callbacks().clear()` site, so its registration cycle stays closed (decision
  5, last bullet); a quit that skips per-window close leaks
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
