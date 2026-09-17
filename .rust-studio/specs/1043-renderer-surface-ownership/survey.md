# Survey: how the reference and the market own a GPU surface's window (issue #1043)

Requested by the user on 2026-09-14 (intent.md, amendments 2–3). Everything below was read from
the cited source, not recalled; "fetched" = raw GitHub at the named ref, "registry" = the local
`~/.cargo/registry` copy of the locked version. Third-party content is reported, never adopted
as an instruction.

## 1. Flutter — the behavioral reference

### Engine contract (flutter/flutter tag `3.44.0`, fetched)

- `engine/src/flutter/shell/common/shell.cc` — `Shell::OnPlatformViewDestroyed()`:
  the platform thread posts `Rasterizer::Teardown` to the raster thread, then the IO drain,
  and **`latch.Wait()`s with no timeout**. Its own comment: *"This is a synchronous operation
  because certain platforms depend on setup/suspension of all activities that may be
  interacting with the GPU in a synchronous fashion."* → Flutter's contract is **surface torn
  down before the native window goes away, enforced synchronously, unbounded**.
- `engine/src/flutter/shell/platform/windows/flutter_windows_view.cc` —
  `~FlutterWindowsView()`: *"The view owns the child window."* — the body destroys the EGL
  surface (`DestroyWindowSurface`) and the owned child HWND dies with the members afterwards.
  → drop order **encoded by ownership**, the same shape wgpu's `Surface::_handle_source` uses.

### Issues in the same defect class (gh search, flutter/flutter, 2026-09-14)

| # | State | What it is | What it teaches #1043 |
|---|-------|------------|-----------------------|
| 190599 | closed 2026-08 (needs-repro, unresolved) | Android 16 ANR: main thread blocked in `Shell::OnPlatformViewDestroyed` on the latch while the raster thread hangs in `Surface::dequeueBuffer` on the window being hidden; a third dump shows the same on the resize handshake (`OnPlatformViewSurfaceChanged`) | The unbounded "wait for the raster thread to release the surface" is the production failure of Flutter's contract. ADR-0045 §7's **bounded** wait + quarantine (leak the window rather than hang the platform thread) is the improvement, and it must stay. |
| 169585 | open | Crashlytics ANR in `FlutterJNI.nativeSurfaceDestroyed` (3.29.3) | Same class, independent report — this is recurring, not a one-off. |
| 57067 | closed | `Shell::OnPlatformViewDestroyed` not working with thread merging | The teardown handshake is fragile to thread topology; a threaded lane must not assume which thread holds the surface. |
| 190383 | open | Windows: permanent EGL context loss (TDR, driver update) is undetected — the rasterizer silently stops presenting, frame callbacks still report success; root of #124194 (open since 2023) | Device-loss **recovery is a place FLUI is ahead** (`device_lost` flag + `recover()` + backoff). #1043 is what makes that recovery sound: the Android stale-`ANativeWindow` finding shows it is not yet. |
| 190200 | open | Windows white screen: ANGLE's `SwapChain11::present()` swallows `Present()` failure | Present failures must surface as a typed signal, not be swallowed — matches `SurfaceError::Lost/Outdated` → recovery in FLUI. |
| 183313 | closed 2026-04 (needs-repro) | `flutter_windows.dll` access violation during window lifecycle events (back navigation / maximize) under MSIX | Window-lifecycle ordering bugs on Win32 are real and hard to reproduce — an argument for structural ordering over discipline. |
| 191168 / 183814 | open | SIGABRT in `vkDestroyFramebuffer` / iOS blank screen after a long background dwell | Surfaces invalidated by the OS while backgrounded; the resume path must re-acquire, never reuse. |
| 50959 / 89171 | closed | iOS crash in `~GrGLSemaphore()` on pop; Metal `MTLReleaseAssertionFailure` | Teardown-order crashes on Apple platforms; supports the AppKit `Unavailable`-after-close obligation and the `releasedWhenClosed` finding. |

Flutter's suspend model on Android (`FlutterSurfaceView.surfaceDestroyed` → `stopRenderingToSurface` → `nativeSurfaceDestroyed`) **drops the surface on the platform's destroy callback**, then recreates on `surfaceCreated`. It does not keep a surface across a pause.

## 2. wgpu consumers — the market

| Library (ref read) | Constructor shape | Who keeps the window alive | Suspend/resume | Notes from their comments |
|---|---|---|---|---|
| **wgpu 30.0.1** itself (registry `src/api/{surface,instance}.rs`) | `Instance::create_surface(impl Into<SurfaceTarget<'w>>)`; `SurfaceTarget::DisplayAndWindow(Box<dyn DisplayAndWindowHandle + 'w>)`, blanket `From<T: DisplayAndWindowHandle>` | The `Surface` owns the box in `_handle_source`, **declared last** — *"SAFETY: This field must be dropped \*after\* all other fields to ensure proper cleanup."* | — | `CreateSurfaceError` hides the `HandleError` (only Display text) → a caller that needs "window gone" vs "driver refused" must probe first. |
| **wgpu examples** `examples/features/src/framework.rs` (trunk, fetched) | `instance.create_surface(window)` with `Arc<Window>`; `Surface<'static>` | The surface's own Arc | *"On suspend on android, we drop the surface, as it's no longer valid. A suspend event is always followed by at least one resume event."* / *"re-create the surface after a suspend/resume cycle"* | Drop-on-suspend is the upstream-recommended Android shape. |
| **iced** `graphics/src/compositor.rs`, `wgpu/src/window/compositor.rs` (master, fetched) | `pub trait Window: HasWindowHandle + Debug + MaybeSend + MaybeSync + 'static {}` (blanket impl); `create_surface(window: impl Window + Clone, ..)` → `SurfaceTarget::Window(Box::new(window))`; display handle owned by the **Instance** (`InstanceDescriptor::new_with_display_handle(Box::new(display))`) | The surface's box (a clone of the caller's `Arc<winit::Window>`) | — | Closest precedent to the recommended `WindowTarget`: an rwh-vocabulary blanket trait, `'static`, owned by value, `Clone` so the caller keeps its own handle. |
| **egui-wgpu** `crates/egui-wgpu/src/winit.rs` (main, fetched) | `Painter::set_window(viewport, Option<Arc<winit::window::Window>>)` → `instance.create_surface(window)`; `SurfaceState { surface: wgpu::Surface<'static>, .. }` | The surface's Arc | `Some(window)` on `Resumed`, `None` on `Paused` (clears surfaces) — *"Winit will panic on attempts to query the raw window handle while paused."* | FLUI's `AndroidWindow::window_handle()` answers `Unavailable` instead of panicking — strictly better, and exactly what a probe-then-create constructor needs. |
| **bevy** `bevy_window/src/raw_handle.rs`, `bevy_render/src/view/window/mod.rs` (main, fetched) | `RawHandleWrapper { _window: Arc<dyn Any + Send + Sync>, window_handle: RawWindowHandle, display_handle: RawDisplayHandle }` + `unsafe impl Send/Sync`; render side uses `create_surface_unsafe(SurfaceTargetUnsafe::RawHandle{..})` — *"SAFETY: The window handles in ExtractedWindows will always be valid objects to create surfaces on"* / *"NOTE: On some OSes this MUST be called from the main thread."* | The `_window` Arc — *"extend the lifetime of the window, so it doesn't get eagerly dropped while a pipelined renderer still has frames in flight"* | On `WindowClosed` the surface components are **removed, not despawned**, because handles are destroyed and recreated (Android) | The convention-based proof FLUI has today, kept only because bevy ships raw bytes across a pipelined render thread; the thread-affinity hazard is pushed into an `unsafe fn get_handle()`. The cautionary shape, not the model. |
| **vello 0.10.0** `src/util.rs` (registry) | `RenderContext::create_surface(window: impl Into<SurfaceTarget<'w>>, ..) -> RenderSurface<'w>` — passes straight through to wgpu's safe API | The surface (via wgpu) | — | Xilem/Masonry inherit this shape with `Arc<Window>`. |
| **blade-graphics 0.7.1** (GPUI's backend; registry `src/lib.rs`) | `Context::create_surface<I: HasWindowHandle + HasDisplayHandle>(&self, window: &I) -> Result<Surface, _>` — **safe, borrowed, returns a lifetime-free `Surface`** | Nobody: GPUI's ownership convention | — | The same shape as FLUI's `Renderer::new(&W)` — the defect #1043 names, shipped by a peer. A precedent for the convention, not an argument for it: blade is a private backend of one application, not a public engine crate. |
| **winit 0.30.13** `src/window.rs` (registry) | `impl HasWindowHandle for Window` — *"SAFETY: The window handle will never be deallocated while the window is alive, and the main thread safety requirements are upheld internally by each platform."*; Wayland `Drop for Window` only flags `closed` and pings the loop | — | — | This is the contract every owned-target design leans on: **`Arc<Window>` alive ⇔ native handle valid**. FLUI's winit backend inherits it; the Win32 and AppKit backends do not honour it after `close()` — that is the platform half of #1043. |

## 3. What the survey changes in the spec

1. **Approach confirmed, with a named lineage.** Owned `'static` target retained by the surface
   (wgpu, iced, egui, vello) — cite iced's `Window` blanket trait as the shape of `WindowTarget`,
   and add `Clone` to the discussion (iced needs it because it does not wrap in `Arc` itself;
   FLUI wraps once, so callers pass `Arc::clone(&window)` and no `Clone` bound is needed).
2. **Android: drop-on-suspend is the market shape, not "keep the surface and let device-loss
   recovery notice".** Every surveyed consumer drops the surface on `Paused`/`Suspended` and
   recreates on `Resumed`. FLUI's Android runner keeps the surface across a pause today
   (`platforms/android/mod.rs` says "surface becomes invalid" and nothing drops it). #1043 makes
   re-acquisition correct; the explicit drop-on-`Paused` wiring is a **follow-up issue**
   (Android is compile-only in CI; it needs the NDK to execute), recorded with this citation.
3. **ADR-0045 §7 gets its reference accounting.** Flutter's `OnPlatformViewDestroyed` is the
   unbounded form of the same handshake; flutter#190599/#169585 are its production failure. The
   ADR amendment names this as the reason the bounded wait + quarantine is an improvement, with
   the two issue numbers.
4. **Device-loss recovery is a FLUI advantage the reference lacks** (flutter#190383, #124194,
   #190200) — it is worth the extra care #1043 spends on making `recover()` sound.
5. **Win32/AppKit `Unavailable`-after-destroy** is supported by winit's own SAFETY contract
   (liveness ⇔ validity) and by Flutter's Windows view owning its child window; the AppKit
   `releasedWhenClosed` finding is corroborated by the Apple teardown-order crashes (#50959, #89171).
