# Scout map — issue #1187

Observations only. Branch `vanyastaff/1187-android-callbacks-clear-site`, HEAD `98665a47`.

## The cycle on Android, edge by edge

`crates/flui-app/src/app/runner/android.rs`, `bootstrap_android`'s registrations on the window:

| Line | Slot | Captures |
|---|---|---|
| 278 | `on_input` | `realm_dispatch` (Copy) |
| 291–443 | `on_request_frame` | **`Arc<RasterLane>`**, `Arc<ScenePlugin>`, `Arc<DeviceRecoveryBackoff>` |
| 447–452 | `on_resize` | `realm_dispatch` |
| 484–486 | `on_close` | nothing (logs) |
| 500–513 | `on_active_status_change` | `realm_dispatch` |
| 581–670 | `on_surface_status_change` | **`Arc<RasterLane>`** |

`RasterLane` owns the `Renderer`, whose `SurfaceLease::target` is the `Arc<dyn WindowTarget>` cloned
from the window at construction (`android.rs:177`). The platform's own field
`window: Arc<Mutex<Option<Arc<AndroidWindow>>>>` (`platforms/android/mod.rs:153–161`) holds a second
strong reference. Two slots close the cycle: `on_request_frame` and `on_surface_status_change`.
`frame_pacing::install_pre_present_hook`: not called on android (unverified beyond a grep of
`android.rs`; the plan should confirm).

## The loop and its exit (`platforms/android/mod.rs`)

- `run()` sets `running = true` (278–290); the loop breaks when `running` reads `false` (292–295).
- `MainEvent::Destroy` arm (401–422): under the `window` guard, `dispatch_close()` on the window,
  then `running = false`. No `clear()`.
- After the loop (494–501): `invoke_quit()`, the bootstrap-error check, return. Nothing touches the
  window. The `window` field is never cleared by any arm; `open_window` (509–517) is its only writer
  and constructs `Arc::new(AndroidWindow::new(..))` on every call.
- Back in `run_android` (`runner/android.rs`, around the `teardown_platform_realm()` call): what runs
  after `platform.run` returns is the realm teardown; whether it dispatches anything to the window is
  **unverified** by the scout and must be read by the plan.

## `WindowCallbacks::clear` (`shared/handlers.rs`)

- `clear()` (535–544) goes through `DispatchDrain::begin(.., WindowCallbackEvent::Clear)`: if a drain
  is already running, `Clear` is queued FIFO and runs when reached; at the top level it runs
  synchronously via `drain_events()` → `clear_now()` (554–575), which sets the `cleared` latch
  (289–301, never reset) before taking all 11 slots.
- `CallbackLease::drop` (422–454) reads `cleared` under the slot lock and drops the callback instead
  of restoring it. A registration made after a clear runs once, then its lease discards it
  (test `a_callback_registered_after_clear_runs_once_then_its_lease_drops_it`, 1036–1058). That is
  the "forbids re-registration" rule.

## How the other backends clear

| Backend | Site | Context |
|---|---|---|
| headless | `headless/platform.rs:780`, `complete_close()` | after `dispatch_close()` and `notify_closed()` (map removal) |
| winit | `winit/platform.rs:1929, 2015`, `complete_window_close()` | on the owner's turn after `on_close`; map entry removed before the clear |
| winit | `winit/window.rs:138`, `Drop for WinitWindow` | last-resort, cannot fire first while the cycle holds |
| macOS | `macos/window.rs:1548`, `handle_close()` | `windowWillClose:`, after the `closed` flag |
| Windows | `windows/platform.rs:751` | `WM_DESTROY` arm, after `dispatch_close()`, HWND still valid |
| android | none | |

Every site runs after `dispatch_close()` and outside any leased callback; every backend removes the
window from its tracking before or during the same sequence. No shared helper; each is inline.

## `android-activity` 0.6.1 recreation

`ANativeActivity_onCreate` (`glue.rs:876`) spawns a new thread per activity instance (`:908`) that
runs `android_main`; `AndroidApp::new` (`mod.rs:67–128`) constructs per-activity state.
`on_destroy` writes `AppCmd::Destroy` (`glue.rs:119, 405`), which only sets `destroy_requested`
(`:637–640`); leaving the loop is FLUI's decision. A recreated activity is a new thread, a new
`AndroidApp`, a new `AndroidPlatform`, a new `AndroidWindow`. The old window survives only through
its own cycle.

## Tests and guards

`handlers.rs` 918–1119 covers `clear()`'s lease and queue semantics on `WindowCallbacks` directly
(six tests). No `Arc::strong_count` or drop-log probe exists on any window; no mechanical guard greps
a backend for a clear site.

## Text to change

- `runner/android.rs:567–579`: the two "idempotent-by-absence" / "Adding that clear is NOT part of
  this change" bullets.
- `docs/adr/ADR-0063-the-renderer-owns-its-surface-target.md:139–142` (the census sentence "none
  under `android/`") and `:143–158` (the Android bullet ending "Filed as a follow-up").
