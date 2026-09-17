# Plan: issue #1187, the Android `callbacks().clear()` site

Branch `vanyastaff/1187-android-callbacks-clear-site`, HEAD `98665a47`, tree clean. Revision 2,
after `reshape-1.md` (both review lenses: ACCEPTABLE with required changes R1 to R5, all adopted
below). This plan is read-only evidence plus decisions; the builder implements. Every claim names
the file and symbol it was read from; `unverified` marks the ones reading could not settle.

## Verdict

**Maintainer-grade pre-code verdict: ACCEPTABLE.** ARCH-GATE passes for the shape below. The site
is a two-statement exit-path release in `flui-platform`, the primitive it calls already exists and
is already tested, and the text sites are the runner's, the window's SAFETY comment, and the
record's. Nothing new is abstracted.

Decisions, one line each (the reasoning is in the sections that follow):

- **D1:** the clear lands on `AndroidPlatform::run`'s exit path, after the `loop` and before
  `invoke_quit()`, not in the `Destroy` arm (option b). Three returning routes reach it (`Destroy`,
  `quit()`, a failed bootstrap); a panic that unwinds out of `run` is a fourth route the site does
  not cover, by decision, and the rejected `Drop` guard is recorded with its cost.
- **D2:** the exit path `take()`s the platform's `window` field before clearing, in its own `let`.
  Nothing after the loop reads it, remove-then-clear is the sibling close bodies' order, and the
  one-window assumption this rests on is stated as an assumption.
- **D3:** a `WindowCallbacks`-level capture-release test (the primitive both backends call) plus a
  function-scoped mechanical guard on `run`'s exit region that strips comments before it masks
  literals and refuses inversion, plus the two Android type-checks. No shared helper; nothing
  executes the site on this host, and the plan says so.
- **D4:** a recreated activity never re-registers on the cleared set: `on_ready` is `FnOnce`, and
  `android-activity` runs one `android_main` per activity instance with a fresh `AndroidApp`. The
  same-window re-registration scenario is unreachable in the shipped runner.
- **D5:** the exit-path clear is at the FIFO's top level (no `poll_events` callback on the stack,
  `DispatchDrain::begin` finds `dispatching == false`), runs outside the `window` lock, and the one
  `Drop` on the dropped chain that takes a lock (`RasterOwner`'s) takes the mailbox's own and fires
  a wake hook nothing on Android installs.

## The cycle, confirmed edge by edge

Read from `crates/flui-app/src/app/runner/android.rs`'s `bootstrap_android`: `on_request_frame`
(step 6) and `on_surface_status_change` (step 8b) each capture an `Arc<Mutex<RasterLane>>`; the
lane owns the `Renderer` (step 4, `RasterLane::new(renderer, ..)`), and `Renderer::new(Arc::clone(&window))`
(step 2) hands the renderer's `SurfaceLease` an `Arc<dyn WindowTarget>` clone of the window
(`crates/flui-engine/src/wgpu/surface_lease.rs`'s `SurfaceLease::target`). The two slots are the
only owners of the lane: the surface applier installed at step 4 holds a `RasterResizeHook`
(`crates/flui-app/src/app/raster_lane.rs`'s `RasterResizeHook`: a mailbox handle and a stamp, no
lane), and the `on_close`, `on_input`, `on_resize`, `on_active_status_change` and quit closures
capture only the `Copy` `realm_dispatch`. `frame_pacing::install_pre_present_hook` is called from
`runner/desktop.rs` only (`rg -n 'install_pre_present_hook\(' crates/flui-app/src`: two hits, the
definition and `desktop.rs`), so on Android the cycle runs through the lane directly. Both review
lenses confirmed there is no third strong holder of the lane.

Other strong references to the `Arc<AndroidWindow>` at loop exit, all released by ordinary
ownership once the cycle is broken: the platform's `window` field (`platforms/android/mod.rs`'s
`AndroidPlatform`), the realm's clone (`UiRealm::new(.., Arc::clone(&window), ..)`, step 3, dropped by
`teardown_platform_realm`'s `drop(realms)`), and `AppRuntime`'s redraw slot (step 9,
`set_redraw_window`, dropped by `teardown_platform_realm`'s `clear_redraw_window()`). The
`AndroidPlatform` itself is kept alive by the `OwnerPlatform` in `APP_RUNTIME.owner_platform`
(`install_owner_platform` inside `on_ready`) and dies when `run_android`'s `OwnerHostClearGuard`
drops at that function's end (`crates/flui-app/src/app/runner/host.rs`'s `impl Drop for OwnerHostClearGuard`).
So on a returning route, with the cycle broken, every reference is gone before `android_main`
returns.

## D1: where the clear lands

**Chosen: (b), the exit path of `AndroidPlatform::run`.** Rejected: (a) the `Destroy` arm alone,
(c) both, and ALT-1, a `Drop` guard armed before the loop.

`run`'s loop has one exit, the top-of-loop `if !platform.running.load(..) { break; }`, and three
returning routes reach it, read from `platforms/android/mod.rs`'s `run`: the `Destroy` arm's
`running.store(false)`; `Platform::quit`'s `running.store(false)` (reachable from any thread through
`SharedPlatform::quit`, `crates/flui-platform/src/traits/owner.rs`); and the `on_ready` `Err` branch's
`running.store(false)` plus `continue`. Only the first passes through the `Destroy` arm. The other
two are real, not theoretical: when `android_main` returns for any reason, `android-activity`
0.6.1's `rust_glue_entry` (`native_activity/glue.rs`) calls `ANativeActivity_finish(activity)`
right after `android_main(app)` returns, so returning from the loop IS how an app finishes its
activity on this backend, and the `Destroy` command the JVM thread later writes
(`notify_destroyed`) goes into a pipe this loop no longer reads. An arm-only clear leaves the cycle
closed on exactly the exit an app chooses for itself. This is the same hole winit closed with
`release_open_window_callbacks` in `finish_shutdown` (`platforms/winit/platform.rs`), whose doc
names `owner.quit()` and a failed `on_ready` as the two routes `complete_window_close` never runs
for. The JVM thread cannot hang on the quit route either: `rust_glue_entry` sets
`thread_state = Stopped` after the finish call, and `set_window`'s park loops only while the state
is `Running`, so a `TermWindow` the glue writes afterwards falls through.

**The fourth route, named and not covered: a panic that unwinds out of `run`.** The same
`rust_glue_entry` wraps the call as `catch_unwind(|| android_main(app)).unwrap_or_else(log_panic)`
and then calls `ANativeActivity_finish`, so a panic anywhere inside the loop (a frame closure, a
widget build, an `expect("BUG: ..")`) unwinds past the exit region, the process survives, the
cycle is never broken, and the next activity's `android_main` runs beside a stranded window, lane
and renderer. No `panic = "abort"` profile exists in the workspace (`rg -n 'panic' Cargo.toml`
finds no profile key), and nothing on the Android frame path isolates panics: the `catch_unwind`
sites in `flui-platform/src` are `shared/panic_boundary.rs` (Win32's window procedure),
`windows/platform.rs`, `macos/clipboard.rs` and winit's real-loop tests, none on Android.

ALT-1 would arm a `Drop` guard before the loop that takes the field and clears on unwind. Rejected,
recorded with its cost: the guard would run `clear()` during unwind, which drops the renderer,
which drops a configured `wgpu::Surface` on the quit-shaped routes, which reaches
`vkDeviceWaitIdle` and `vkDestroySurfaceKHR` on a device that may be the reason for the panic; a
second panic there aborts the process and defeats the graceful finish the glue's `catch_unwind`
exists to provide. `OwnerHostClearGuard`'s doc (`runner/host.rs`) makes the same double-panic
argument for keeping its own `Drop` to a single `take()`, and winit's `finish_shutdown` has the
identical boundary (it runs only after `run_app` returns; a panic out of the loop never reaches
`release_open_window_callbacks`). Whether wgpu-hal's Vulkan release path can itself panic on
`VK_ERROR_DEVICE_LOST` is `unverified`; the decision does not depend on it, because the abort
hazard is the second panic, whatever raises it. Text rule for every site this plan touches: "three
returning routes, plus one named exception"; never "every route".

Option (c) adds nothing: `clear()` is idempotent-terminal, so a second clear on the exit path is a
no-op after an arm clear, and two sites are two places to keep in step. The `Destroy` arm gains no
ordering the exit path lacks either. Win32 clears inside `WM_DESTROY` because the HWND is still
valid there and gone after; on Android the native window is already gone by `Destroy` (below), so
there is no "while the handle is still valid" moment the arm would protect.

**What runs after the clear, and whether it needs anything the clear drops.** In order: the tail of
`run` (`invoke_quit()`, the bootstrap-error check), then `run_android`'s `teardown_platform_realm()`,
then the `Err` panic check, then the `OwnerHostClearGuard` drop. `invoke_quit` reaches the runner's
quit hook, which dispatches `Lifecycle(Detached)` into the realm through `realm_dispatch` (a `Copy`
address, not a callback); the realm lives in `APP_RUNTIME`, not in any slot.
`teardown_platform_realm` (`crates/flui-app/src/app/runner/realm_dispatch.rs`) removes and drops
the realms, shuts services and pools down, and clears the clipboard and redraw slots; it dispatches
nothing to the window. Neither needs the lane, the renderer, or any closure. A queued
`RealmTask::Frame` that captured a lane clone would be dropped with its realm, so even that corner
ends the same way.

**Order relative to `invoke_quit`: clear first.** Same order as winit's `finish_shutdown`
(`release_open_window_callbacks`, then `close_owner_lane`, then `notify_quit_once`), and for a
reason that survives the analogy: `invoke_quit` runs embedder code (the Detached lifecycle
transition and every listener behind it) under `platform.handlers.lock()`; if that code panics, the
unwind leaves `run` with the cycle still closed unless the clear already ran.

**Surface state on each returning route, verified.** On the `Destroy` route the surface was already
released by the `TerminateWindow` arm: AOSP's `NativeActivity.onDestroy` calls
`onSurfaceDestroyedNative` before `unloadNativeCode` (fetched 2026-09-16 from
`android.googlesource.com`, `refs/heads/main`, `core/java/android/app/NativeActivity.java`, lines
188 to 200; not a pinned tag, so treat as the current shape rather than a frozen one), and
`android-activity`'s `set_window(None)` (`native_activity/glue.rs`, called from
`on_native_window_destroyed`) parks the JVM thread until the Rust loop has applied `TermWindow`, so
the `TerminateWindow` callback has fully returned before `Destroy` is even written. The renderer
dropped by the clear then holds no `wgpu::Surface`. On the quit and bootstrap-error routes no
`TerminateWindow` was delivered, the `ANativeWindow` is still live, and the clear drops the
configured surface on the loop thread while its window exists, the order issue #713 requires. That
drop pays `vkDeviceWaitIdle` on the owner thread once, the same cost `ensure_surface`'s release
already pays inside `TerminateWindow` (`runner/android.rs`, step 8b's comment); `unverified` on
hardware, same as every other Android claim in this repository.

## D2: the platform's `window` field

**Chosen: `take()` it on the exit path, before the clear.** Rejected: leave it and let the platform's
own drop release it.

Nothing after the loop reads the field: `run`'s tail touches `handlers` and `bootstrap_error` only;
`invoke_quit` and `teardown_platform_realm` never reach the platform's window; `active_window()`
has no post-exit caller in `flui-app` (`rg -n 'active_window\(' crates/flui-app/src` finds desktop
paths only). So leaving it would also be correct once the cycle is broken, because the
`OwnerHostClearGuard` drops the platform at `run_android`'s end. `take()` is chosen because on this
backend the exit path is the window's whole close body: the sequence the winit and headless close
bodies run (`complete_window_close`: `on_close`, removal from tracking, then `clear`;
`complete_close`: `dispatch_close`, `notify_closed`'s map removal, then `clear`) maps onto Android
as `dispatch_close` in the `Destroy` arm, `take()`, `clear()`. With the field taken, A3 holds by
construction rather than by inspection: no arm and no tail can find a window to dispatch to after
the release. The one consequence is that `active_window()` answers `None` after exit, which is the
truthful answer.

**The assumption this rests on, stated as one: one window per platform.** `OwnerPlatform::open_window`
overwrites the field (`*self.window.lock() = Some(..)`) without clearing the displaced window's
callbacks, so a second `open_window` during the loop would leave the exit path clearing only the
latest window and the displaced one's cycle closed. No caller opens a second one today:
`bootstrap_android` calls `open_window` once, and `open_secondary_window` is
`cfg(not(target_os = "android"))` (`runner/realm_dispatch.rs`, the gated call site). Pre-existing,
not worsened by this change, and an assumption rather than an invariant.

The `take()` must be its own `let` statement so the guard drops before `clear()` runs: in edition
2024 an `if let` scrutinee's temporaries live through the `if` body, and the clear must not run
under the `window` lock (D5). The mechanical guard refuses the fused shape (D3).

## D3: pinning A1 when the arm cannot execute here

**Chosen:** a `WindowCallbacks` capture-release test in `shared/handlers.rs`, a function-scoped
source guard in `crates/flui-platform/tests/`, and both Android type-checks. **Rejected:** (i) a
shared helper in `shared/` that the exit path calls and a host test drives, and (iii) type-check
only.

(i) collapses on inspection. The exit path is `slot.lock().take()` then `WindowCallbacks::clear()`;
a helper wrapping those two lines has one production caller and exists so a test can name it, and
the test would prove "the helper clears" while "Android calls the helper on its exit path" still
needs the source guard. So (i) is (ii) plus an abstraction, which the maintainer rejection test
names directly. `WindowCallbacks::clear` is already the shared primitive both backends call; that
is what A1's "shared helper both backends call" resolves to. (iii) is rejected because a type-check
proves the call compiles, not that it is there: a later edit that deletes the two lines stays green
under every gate, which is the exact failure A1's guard clause exists to prevent.

**What the existing tests do and do not pin.** The six tests in `handlers.rs` (`shared::handlers::tests`,
run 2026-09-16: 6 passed) assert slot emptiness or single-fire. The `on_request_frame` `is_none()`
assertion in `close_from_inside_a_drained_callback_still_fires_on_close` is vacuous for that slot
(the test never registers it), so a `clear_now` that forgot `on_request_frame` is caught by nothing
today; `on_surface_status_change` is already pinned by
`surface_status_change_cleared_from_inside_is_not_resurrected`. Capture release specifically is
pinned on two other links of the chain and not on this one: `surface_lease.rs`'s
`dropping_the_lease_drops_the_only_strong_target_ref` is a `Weak` probe on the lease-to-target
link, and winit's `frame_callback_owner_is_released_by_complete_window_close`
(`platforms/winit/platform/real_loop_tests.rs`) probes the callback link through a real event loop.
A `WindowCallbacks` test is the shallowest tier that can fail for the claim Android's site rests on.

**Red-to-green the builder owes, per artifact:**

1. *Guard, the true red for this change.* Write `tests/android_exit_path.rs` first and run it
   against the unmodified `platforms/android/mod.rs`: red, with the message naming the missing
   exit-path clear. Land the site: green. Then five mutants, each restored afterwards, each red:
   (c) move the two statements into the `Destroy` arm (outside the exit region); (d) delete the
   `clear()` line only; (e) delete the code line and leave `window.callbacks().clear()` in a
   comment on the same spot (comments are stripped before anything is matched); (f) fuse the two
   statements into `if let Some(w) = platform.window.lock().take() { w.callbacks().clear(); }`
   (test 2 refuses a take line that is not a `let` statement); (g) rename the binding on one line
   only (receiver no longer matches). Report every verdict with the command.
2. *Handlers test, a discrimination proof, not a red-before-fix.* It is green on HEAD, because
   `clear()` already releases captures; its value is that it pins the sequence Android performs
   (top-level dispatches, then `dispatch_close`, then `clear`) on the two cycle-closing slots.
   Three mutants: remove `self.on_request_frame.lock().take()` from `clear_now`'s tuple (red on the
   frame probe; this is the first real pin of that take); remove
   `self.on_surface_status_change.lock().take()` (red on the surface probe, and already red under the
   existing resurrection test, so no new discrimination is claimed for it); replace
   `drop(dropped)` in `clear_now` with `std::mem::forget(dropped)` (red on both probes, green on
   every existing slot-emptiness test: this is the mutant only a `Weak` probe kills, and the reason
   the test exists). Restore: green.
3. *Type-checks.* The Android `flui-app` check (7 pre-existing warnings on HEAD, measured
   2026-09-16: `field presented is never read`, `desktop_secondary_wake_deadline never used`,
   `capacity`/`command_sender`/`get` never used, one unfulfilled lint expectation, four unconstructed
   `PlatformToUi` variants; the diff must add none) and the `cross-typecheck` Android line (green on
   HEAD, 2.0 s warm, `-D warnings`, so the site must be clippy-clean).

## D4: the re-registration rule

`clear()` latches `cleared` (`handlers.rs`'s `clear_now`), and `CallbackLease::drop` discards
instead of restoring once it is set, so a registration made after the clear runs once and is gone
(`a_callback_registered_after_clear_runs_once_then_its_lease_drops_it`). For that to bite, something
would have to register on the same `WindowCallbacks` after the exit path.

A recreated activity cannot: `ANativeActivity_onCreate` (`android-activity` 0.6.1,
`native_activity/glue.rs`) constructs a new `NativeActivityGlue` and spawns a new thread running
`rust_glue_entry`, which builds a new `AndroidApp` (`AndroidApp::new`, `native_activity/mod.rs`) and
calls `android_main(app)` afresh; the crate's own `lib.rs` doc states it ("a new `AndroidApp` will
be created for that new `Activity` instance and sent to a new call to `android_main()`"). A new
`android_main` runs `run_app_android` again, which constructs a new `AndroidPlatform::new(app)`,
whose `open_window` does `Arc::new(AndroidWindow::new(..))` with a fresh `WindowCallbacks`
(`platforms/android/window.rs`'s `AndroidWindow::new`). The old window object is never seen again.

The one scenario that would re-register on the same object is a second `bootstrap_android` in the
same `android_main`. It is unreachable: `bootstrap_android` is called only from the `on_ready`
closure, `on_ready` is `PlatformReadyCallback` (`FnOnce`), and `run` calls `on_ready.take()` exactly
once. After the loop exits nothing in `run` or `run_android` registers. An embedder that kept the
`Arc<dyn PlatformWindow>` `open_window` returned and registered on it after `run` returned would
hit the once-then-discarded rule, which `clear()`'s doc already names as a path no backend should
use; that is a misuse, not a lifecycle.

## D5: the ordering hazards

**Top level, confirmed from `DispatchDrain::begin`.** `begin` pushes the event and returns
`Some(drain)` only when `dispatching` is `false`; `next()` resets `dispatching` to `false` when the
queue empties, before returning `None`, and `drain_events` loops until then. The `Destroy` arm's
`dispatch_close()` runs from `poll_events`' callback, which is not a `WindowCallbacks` drain, so it
gets the drain, fires `on_close`, and returns with `dispatching == false`. The exit path runs after
`poll_events` has returned and after the loop's own input and frame dispatches have returned, so no
`CallbackLease` is live and `clear()`'s `begin` runs `clear_now` synchronously. A4 holds by
position, not by luck. No other thread dispatches on this backend (every `dispatch_*` call in
`platforms/android/mod.rs` is in `run` or `process_input_events`, both loop-thread).

**What the drops lock, read rather than grepped.** `clear_now` takes the eleven slot guards for one
`let` statement, releases them, then drops the callbacks. The dropped closures own:
`Arc<Mutex<RasterLane>>` (the last two clones, so `RasterLane` drops, then its `RasterOwner`, then
`Renderer`, then `SurfaceLease`, whose `Drop` logs one `tracing::debug!` and drops its
`Option<Surface>` and its `Arc<dyn WindowTarget>` clone), `Arc<DeviceRecoveryBackoff>`, `ScenePlugin`
(a unit struct without the `hot-reload` feature; `Arc<Mutex<HotReloadDriver>>` with it), and
`realm_dispatch`. One `Drop` on that chain takes a lock: `impl<B: RasterBackend> Drop for RasterOwner<B>`
(`crates/flui-engine/src/raster_owner.rs`) locks the mailbox's own `state` mutex to take an
orphaned pending frame (the guard is a statement temporary, released before the next line) and
then calls `accounting.notify_retired()`, which fires the mailbox wake hook if one is installed.
The lock is the mailbox's, not the platform's, and the hook is `None` on Android: it is set only by
`RasterHandle::set_wake_hook`, whose every caller is inside `raster_owner.rs`'s `mod tests`
(`rg -n 'set_wake_hook' crates --type rust`: the definition, two doc mentions, and test-module
calls only). `AndroidWindow` and `WindowCallbacks` have no `Drop`, and `AndroidPlatform`'s `window`
and `handlers` mutexes are private fields nothing outside the platform can name. Running the clear
under the `window` guard would therefore not deadlock today; it still runs outside it, because it
drops embedder closures and the file's own discipline (ADR-0038 §5, cited at `run`'s wake-deadline
read) is to clone or take out of a platform lock before running embedder code.

**Two pre-existing hazards observed nearby, not in scope, to be filed rather than fixed here:**
`invoke_quit()` runs under `platform.handlers.lock()` (winit leases the hook out first), so a
Detached listener that registers a hook would deadlock; and every arm in `run` dispatches under the
`window` guard, so an `on_close` or `on_resize` callback that called `open_window`/`active_window`
would deadlock. Both are uniform across the file; fixing one arm would be a half-ripple.

## Files to change

1. **`crates/flui-platform/src/platforms/android/mod.rs`** (the site). In `run`, between the
   `loop`'s closing brace and `platform.handlers.lock().invoke_quit();`, insert the release with its
   rationale comment; add one `tracing::debug!` naming the release (useful in logcat, which is the
   only observation channel this backend has). Update the module doc's architecture diagram so the
   `Destroy` line reads `-> break loop` and a new line after the loop reads
   `-> loop exit -> window released, callbacks cleared, quit hook`. Shape:

   ```rust
           }

           // The loop's one exit, reached by its three returning routes: a
           // `MainEvent::Destroy`, a `quit()` from any thread, a bootstrap
           // that returned `Err`. A panic that unwinds out of this function
           // skips it, by decision (ADR-0063 decision 5): a clear during
           // unwind would drop a configured surface on a device that may be
           // the panic's cause, and a second panic there aborts the process
           // instead of letting `android-activity`'s `catch_unwind` finish
           // the activity. This is where the window this backend tracked is
           // released, in the order the winit and headless close bodies use:
           // the platform's own reference first, then the callback slots.
           //
           // The slots are the platform's only owning path into the
           // embedder's presentation. `flui-app` registers a frame callback
           // and a surface-status callback that own the raster lane, which
           // owns the renderer, whose surface lease holds an `Arc` of this
           // window (ADR-0063 decision 5): window -> slot -> closure -> lane
           // -> renderer -> `Arc<AndroidWindow>`. Only a clear breaks that
           // cycle; without it every activity recreation strands one window,
           // lane and renderer for the process's life.
           //
           // After the loop rather than in the `Destroy` arm, because
           // `Destroy` is not the only way out: a `quit()` returns from
           // `android_main`, and `android-activity` then finishes the
           // activity without ever delivering `Destroy` here. Before
           // `invoke_quit`, so a quit hook that panics still leaves the cycle
           // broken. Outside the `window` lock, because it drops embedder
           // closures (ADR-0038 §5); the `take()` is its own statement so
           // its guard is gone before the clear runs. At the top level of
           // the window's FIFO, because no `poll_events` callback is on the
           // stack here, so no lease can restore what it takes (issue #919).
           //
           // The surface is safe on every returning route. On `Destroy` it
           // is already gone: `NativeActivity.onDestroy` destroys the surface
           // before it unloads the native code, and `android-activity`'s
           // `set_window(None)` parks the JVM thread until `TermWindow` has
           // been applied, so the `TerminateWindow` arm above released it
           // before `Destroy` was even written. On a `quit()` the native
           // window is still live, so the surface dies here, before it,
           // which is the order issue #713 requires.
           //
           // No registration can land on the cleared set afterwards:
           // `on_ready` is `FnOnce`, so the bootstrap runs once per platform,
           // and a recreated activity gets a new `android_main`, a new
           // `AndroidApp`, a new platform and a new window
           // (`ANativeActivity_onCreate` spawns one thread per activity).
           let window = platform.window.lock().take();
           if let Some(window) = window {
               window.callbacks().clear();
               tracing::debug!("Android: loop exited; window released and its callbacks cleared");
           }

           // Invoke quit handlers
           platform.handlers.lock().invoke_quit();
   ```

   Inline, not a `finish_shutdown` method: `run` has exactly one post-loop tail and one caller of
   it, and a named method would need the guard to pin two things (the sequence, and that the call
   sits after the loop) instead of one. Extract when a second exit path exists.

2. **`crates/flui-platform/src/shared/handlers.rs`** (doc and the primitive's pin). In `clear()`'s
   doc, the enumerated call-site list is a hand-maintained completeness claim ("finds every call
   site"); add the Android site to it so the `rg` census and the prose agree: "the Android
   backend's loop exit (`platforms/android/mod.rs`, `AndroidPlatform::run` after its `loop`,
   reached by `Destroy`, `quit()` and a failed bootstrap; a panic out of `run` skips it by
   decision)". Add the test to `mod tests`:

   ```rust
   /// The sequence the Android backend's exit path performs, pinned on the
   /// primitive it calls: both cycle-closing slots (`on_request_frame` and
   /// `on_surface_status_change` own the raster lane in `flui-app`'s
   /// wiring) are dispatched at the top level during the loop, their leases
   /// restore them, `dispatch_close` consumes `on_close`, and then `clear()`
   /// must drop both closures and with them everything they own. Two probes,
   /// one per slot, so a `clear_now` that forgets either slot fails on a
   /// named assertion. Slot emptiness is not the claim: a `clear_now` that
   /// took every slot and then `mem::forget` the tuple would leave every
   /// slot `None` and every capture alive, and only these probes see that.
   #[test]
   fn clear_after_top_level_dispatches_releases_what_the_cycle_closing_slots_own() {
       let callbacks = WindowCallbacks::new();
       let frame_owned = Arc::new(());
       let frame_weak = Arc::downgrade(&frame_owned);
       callbacks.on_request_frame.lock().replace(Box::new(move || {
           let _ = &frame_owned;
       }));
       let surface_owned = Arc::new(());
       let surface_weak = Arc::downgrade(&surface_owned);
       callbacks.on_surface_status_change.lock().replace(Box::new(move |_has_surface| {
           let _ = &surface_owned;
       }));

       callbacks.dispatch_request_frame();
       callbacks.dispatch_surface_status_change(false);
       callbacks.dispatch_close();
       assert!(frame_weak.upgrade().is_some(), "an ordinary dispatch restores the frame callback");
       assert!(surface_weak.upgrade().is_some(), "an ordinary dispatch restores the surface callback");

       callbacks.clear();

       assert!(frame_weak.upgrade().is_none(), "clear() must drop the frame callback and what it owns");
       assert!(surface_weak.upgrade().is_none(), "clear() must drop the surface callback and what it owns");
   }
   ```

3. **`crates/flui-platform/tests/android_exit_path.rs`** (new) registered in `tests/main.rs` with
   `#[path = "android_exit_path.rs"] mod android_exit_path;`, following the `platform_it`
   single-binary convention. The precedent for a source scan is
   `crates/flui-app/tests/runner_frame_ordering.rs`; follow its honesty header. Specification:

   **Input.** `include_str!("../src/platforms/android/mod.rs")`, so a moved file fails to compile
   rather than scanning nothing.

   **Preprocessing, in this order.** (1) Strip `//` to end of line on every line. `run` holds ten
   comment lines with apostrophes (`` `run`'s ``, `loop's`, `winit's own`), and a char-literal
   masker that ran first would swallow from one apostrophe to the next and take the `poll_events`
   closure's braces with it, mis-locating the loop's close. (2) Mask the contents of `"..."` string
   literals (honouring `\"`) and `'x'`/`'\x'` char literals matched strictly with their closing quote
   in place, keeping the delimiters. Strict, never "swallow to the next `'`": the file's code lines
   carry lifetimes (`Formatter<'_>` on two `Debug` impls, `&'static str` in `name`) and no char
   literals at all, so a lax masker eats from the first lifetime through the `fn run(self: Box<Self>`
   signature and reports "could not locate" for the wrong reason. (3) Walk braces on
   the result. A `//` inside a string literal in `run` would be mis-stripped by step 1; `run` has
   none today, and the anchors below turn such a mis-strip into an explicit failure, never a pass.

   **Locating the function and the region.** Find `fn run(self: Box<Self>` exactly once (zero or
   more than one: fail with "could not locate `AndroidPlatform::run`; read by hand"). Take the body
   from the first `{` after that signature to its matching `}` by depth. Within the body, at body
   depth 0: exactly one line whose trimmed text is `loop {` (else fail, explicit), its matching
   close (the first later line at which depth returns to 0), and exactly one line containing
   `invoke_quit()` (else fail, explicit) that comes after that close (else fail, explicit). The exit
   region is the lines strictly between the loop's close and the `invoke_quit()` line.

   **Inversion refusal, asserted before either shape test.** The region contains no `MainEvent::`,
   no `=>`, no `poll_events` and no `return`, and holds at most 20 non-blank lines. `return` is banned
   because an early `return Err(..)` between the take and the clear would keep both lines and lose the
   clear and `invoke_quit()` on the error route; the real tail's `return` sits after `invoke_quit()`,
   outside the region, so the ban costs nothing. If a masking defect shifted
   the loop's close upward, the region would swallow the `Destroy` arm and the guard would degrade
   to a file-scoped `contains`; these assertions fail it instead.

   **Test 1, `android_run_clears_the_window_callbacks_between_its_loop_and_the_quit_hook`.** Assert the
   clear count first, and only then extract test 2's binding, so that on an unmodified tree the red is
   the explicit "missing exit-path clear" message rather than an index panic about the absent take
   line. Exactly
   one region line contains `.callbacks().clear()`; that line contains no `.lock()`; the receiver
   (the identifier immediately before `.callbacks()`) equals the binding test 2 extracts.

   **Test 2, `android_run_releases_its_own_window_reference_before_clearing`.** Exactly one region
   line contains `.window.lock().take()`; trimmed, it starts with `let ` and ends with `;`; its
   binding is the identifier between `let ` and ` =`; it precedes the clear line. This refuses the
   fused `if let Some(w) = platform.window.lock().take() { w.callbacks().clear(); }`, whose scrutinee
   guard would live through the clear.

   **Header.** The scan proves the two statements are present in the exit region of `run`, in that
   order, in the unfused shape; it proves nothing about what a device executes, and it cannot see a
   conditional wrapper that keeps both lines and loses a route (an `if bootstrap_error.is_none()`
   around them would pass). "Present in the region", never "reached". If `run` gains a construct the
   preprocessing does not handle, the walk must report "could not locate", not pass.

4. **`crates/flui-app/src/app/runner/android.rs`**, step 8b's two bullets (the "idempotent-by-absence"
   and "Adding that clear is NOT part of this change" paragraphs). Replace with the mechanism:

   ```
   // Two more invariants this registration rests on, stated where it is
   // made rather than assumed:
   //
   // * The slots are cleared exactly once, on `AndroidPlatform::run`'s
   //   exit path (`flui-platform`'s `platforms/android/mod.rs`, after the
   //   loop and before the quit hook), on each returning route out of the
   //   loop: `MainEvent::Destroy`, a `quit()`, a bootstrap that failed. A
   //   panic that unwinds out of `run` skips it, by decision (ADR-0063
   //   decision 5). The clear drops this closure and the frame closure
   //   above, which are the only owners of `lane`, so the renderer and its
   //   surface lease go with them and the window's `Arc` is released.
   // * Nothing registers on the cleared set afterwards. `on_ready` is
   //   `FnOnce`, so this bootstrap runs once per platform, and a recreated
   //   activity is a new `android_main` with a new `AndroidApp`, a new
   //   platform and a new window (`android-activity` 0.6.1,
   //   `native_activity/glue.rs`'s `ANativeActivity_onCreate`); the
   //   once-then-discarded rule `WindowCallbacks::clear` documents is
   //   never reached by this runner.
   ```

5. **`crates/flui-platform/src/platforms/android/window.rs`**, the SAFETY comment under
   `window_handle`. Its sentence "`AndroidPlatform` keeps holding its `AndroidWindow` across a
   pause, nothing clears that field on a termination, so `self` and the borrow outlive the
   pointer" gains a second writer of the field after D2. The argument stands, because the
   pointer's bound is the `ANativeWindow` refcount and no `window_handle` call follows the loop, so
   amend the sentence to: the field is written by `open_window` and taken exactly once, on `run`'s
   exit path after the last dispatch, so for every handle this method ever returns `self` and the
   borrow still outlive the pointer. Keep the conclusion. This is the one other place in the backend
   asserting the field is never cleared, in the claim family this backend's defects have come from.

6. **`docs/adr/ADR-0063-the-renderer-owns-its-surface-target.md`**, three passages, edited in
   place with a dated note in the repository's amendment style (`*Amended 2026-09-16 (#1187):*`):
   - Decision 5, the census bullet: both its list (`platforms/{macos,windows,winit,headless}` all
     do) and its census sentence ("Run today it prints hits under ... and none under `android/`.
     Android has none, so the registration cycle stays closed there; that gap is a boundary of this
     record ...") now name all five backends; drop the boundary sentence.
   - Decision 5, the bullet "**The one registration cycle that is still closed is Android's.** ...
     Filed as a follow-up.": replace with the decision (exit path, not the `Destroy` arm; the three
     returning routes and why the `quit()` route forces the exit path; `TerminateWindow` fully
     precedes `Destroy`, with the AOSP and `android-activity` citations above; `on_ready` is
     `FnOnce` and recreation is a new `android_main`); the named exception (a panic that unwinds
     out of `run`, with ALT-1's double-panic cost and the `unverified` wgpu-hal release-path
     question); the evidence line (a `WindowCallbacks` capture-release test, a function-scoped
     guard on the exit region, type-checked by `cross-typecheck` and the NDK-free `flui-app` check,
     executed by nothing on this host); and the forward shape the site is interim under, ALT-2: the
     lane owned by the realm slot and dropped by `teardown_platform_realm` on every returning route,
     with the window closures holding only a handle, which ADR-0045 and issue #559 already point at
     and which is out of scope here.
   - Consequences, the "Still open, named rather than absorbed" bullet: remove the clause "the
     Android backend still carries no `callbacks().clear()` site, so its registration cycle stays
     closed (decision 5, last bullet);" and keep the Win32 and quit-route clauses. Add the panic
     route as a named open item on Android, with its reason.
   - Optional, for coherence: the positive consequence "is broken at native close, not only on
     winit" may add "and on Android at loop exit".

   Citations in the ADR use the file-plus-symbol form the citation checker resolves
   (`scripts/check-adr-citations.py`'s `SYMBOL_CITE`): `` `platforms/android/mod.rs`'s `run` ``,
   never a line number.

Not changed, deliberately: `crates/flui-platform/ARCHITECTURE.md` (no clear-site inventory there;
the record is the ADR), `docs/runtime-contract.toml` (no monitored export moves), the `Destroy`
arm itself, `AndroidWindow`'s derived `Clone` (shares the callbacks `Arc`, odd but unused), and
`active_window()`'s `WindowId(0)` against `AndroidWindow::id`'s `WindowId(1)` (a pre-existing
mismatch, recorded here for a follow-up).

## Gate order

Run from the worktree root, each step separately:

1. `just fmt-check`
2. `just clippy`
3. `FLUI_HEADLESS=1 cargo nextest run -p flui-platform --all-features` (the guard red on HEAD first,
   then green, then its five mutants; the handlers test and its three mutants)
4. `cargo nextest run -p flui-app` (comment-only change in the runner; sanity)
5. `env CC_aarch64-linux-android=clang AR_aarch64-linux-android=ar cargo check -p flui-app --locked --target aarch64-linux-android`
   (7 warnings on HEAD; no new ones)
6. `just cross-typecheck` (not part of `just ci`; the Android line is the one that compiles the
   site and the window's SAFETY comment under `-D warnings`)
7. `just ci`

`just cross-typecheck` runs outside `just ci` (`justfile`: `ci: gate test-ci test-doc`, and `gate`
does not list it), so step 6 is not optional. The project gate is `just ci` at full intensity;
review is `rust-reviewer` after the builder, and `qa-lead` may weigh in on the guard's mutants.

## Risks

- A panic that unwinds out of `run` leaves the cycle closed for that activity, by decision (D1).
  The process survives it today only because the glue catches the panic; the stranded window, lane
  and renderer live until the process ends. ALT-1's cost is the reason this is recorded rather than
  guarded; ALT-2 is the shape that closes it structurally.
- The quit-route surface drop pays `vkDeviceWaitIdle` on the owner thread at exit. Same cost the
  `TerminateWindow` release already pays; `unverified` on hardware.
- The guard is textual. A restructuring of `run` (extracting the loop, renaming `invoke_quit`)
  fails it into an explicit "could not locate" state, which is the intent; the editor updates the
  guard with the restructure. It cannot see a conditional wrapper that keeps the lines and loses a
  route.
- `on_close` still never fires on the quit route, as on winit's quit path. Whether
  `PlatformWindow::on_close` is owed on a `quit()` exit is a cross-backend contract question, not an
  Android decision; named here, not changed.
- The Android `flui-app` check has 7 pre-existing dead-code warnings; a reviewer reading the
  output must not attribute them to this diff, and the diff must not add an eighth.
- `docs/runtime-contract.toml`'s `run_app_android` entry still says `on_ready` fires at the first
  `Resume` (it fires at the first `InitWindow` since #1186). Stale, out of scope, worth a follow-up.

## Maintainer-grade pre-code verdict, itemized

1. **Owning crate.** The site and the SAFETY comment are `flui-platform`'s (`platforms/android/`);
   the runner comment is `flui-app`'s; the record is ADR-0063. No logic crosses a crate boundary.
2. **Sibling primitives.** `WindowCallbacks::clear` (`shared/handlers.rs`) is the one primitive, and
   every other backend calls it inline at its own destroy or exit site (`headless/platform.rs`'s
   `complete_close`, `windows/platform.rs`'s `WM_DESTROY` arm, `macos/window.rs`'s `handle_close`,
   `winit/platform.rs`'s `complete_window_close` and `release_open_window_callbacks`). None
   generalises into a helper Android could call: each is two to four backend-specific steps around
   the same `clear()`. Inline is the repository's shape; the plan keeps it.
3. **Reuse.** `Option::take` and `WindowCallbacks::clear`; nothing new. The `clear()` doc's contract
   ("backends call this at window close") holds: on Android the loop's exit is the window's close.
4. **Classification.** Cross-crate runtime behaviour on a lifecycle path; no public API, no unsafe
   code added (one SAFETY comment amended, its argument unchanged), no hot path. Semver: none
   (`AndroidPlatform` is `pub`, the change is inside `run`).
5. **What a strict maintainer would reject:** a `Destroy`-arm-only clear (misses the quit route); a
   `Drop` guard for the panic route (double-panic abort); a shared helper for testability; a
   file-scoped `contains` guard, or one that masks literals before stripping comments; a clear under
   the `window` guard or the fused `if let` that holds it; "every route" anywhere in the text;
   leaving the runner's "idempotent-by-absence" bullets, the window's "nothing clears that field"
   sentence, or the ADR's "Filed as a follow-up" text in place (half-ripple); a drive-by
   `dispatch_close()` on the quit route; reporting the handlers test as the site's red-to-green, or
   the surface mutant as new discrimination.
6. **Active-dev latitude.** None needed; no compatibility surface is touched.
7. **Sources checked.** `android-activity` 0.6.1 source in the cargo registry
   (`native_activity/glue.rs`: `ANativeActivity_onCreate`, `rust_glue_entry` including its
   `catch_unwind` and `ANativeActivity_finish`, `set_window`, `notify_destroyed`, `pre_exec_cmd`;
   `native_activity/mod.rs`: `AndroidApp::new`, the `poll_events` `Destroy` mapping; `lib.rs`: the
   `MainEvent::Destroy` and recreation doc); AOSP `NativeActivity.java` at `refs/heads/main`
   (fetched 2026-09-16); `crates/flui-engine/src/raster_owner.rs`'s `Drop for RasterOwner<B>` and
   `set_wake_hook`; the workspace files named above.

ARCH-GATE checklist: boundaries sound and acyclic (platform site, app comment, ADR record); the
governing record is ADR-0063 decision 5, amended in place; no layering violation (the platform
drops embedder closures through the primitive it already owns, and never names `flui-app`); public
surface unchanged; implementable by `rust-builder` as scoped; reuse verified (no new primitive); one
fact one place (the census list in `clear()`'s doc is the one enumeration, kept true); no struct
split, no new crate; the forward view is ALT-2, the realm-owned lane, under which the Android site
becomes redundant and the panic route closes structurally, and until then a second Android exit
path, if one ever exists, extracts `finish_shutdown` from this tail and moves the guard's anchor
with it.

## Delegation

`rust-builder`, under this plan, in this order: guard (red on HEAD), site (green), the guard's
five mutants, handlers test plus its three mutants, runner comment, window SAFETY comment, ADR
passages, then the gate order above with every verdict quoted. `rust-reviewer` afterwards; the
review must ask which copy each red was measured against, and must read the exit region as
written rather than trusting the guard for the fused-shape refusal.
