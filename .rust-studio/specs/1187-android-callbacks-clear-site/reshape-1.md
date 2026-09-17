# Reshape 1 — what the plan review found, and the rulings

Two independent lenses returned ACCEPTABLE with required changes and converged on the first finding.
The design survives: the exit-path site, take-then-clear outside the lock, the `WindowCallbacks`
test, the textual guard. What changes is D1's completeness claim, the guard's specification, two
evidence claims, one text site, and the acceptance wording (already corrected in `acceptance.md`).

## Required

### R1. A fourth route exists: panic-unwind. Name it; do not guard it.

`android-activity` 0.6.1 `glue.rs:976–993`: `catch_unwind(|| android_main(app)).unwrap_or_else(log_panic)`
then `ANativeActivity_finish`. A panic anywhere inside `run` (a frame closure, a widget build, an
`expect("BUG…")`) unwinds past the exit region, the process survives, and the next activity's
`android_main` runs beside a cycle nobody will break. No `panic = "abort"` profile exists and nothing
on the Android frame path isolates panics (`catch_unwind` appears only in Win32's
`shared/panic_boundary.rs`).

**Ruling: record, do not adopt a `Drop` guard.** A guard armed before the loop (ALT-1) would run
`clear()` during unwind, which drops the renderer, which drops a configured `wgpu::Surface`,
which reaches `vkDeviceWaitIdle` and `vkDestroySurfaceKHR` on a device that may be the reason for
the panic; a second panic there aborts the process, defeating the graceful finish the glue's
`catch_unwind` exists to provide. `OwnerHostClearGuard`'s own doc in `runner/host.rs` gives the
same double-panic reasoning, and winit's `finish_shutdown` has the identical boundary. The plan
records ALT-1 with that cost and `unverified` for whether wgpu-hal's `release_resources` can panic
on `VK_ERROR_DEVICE_LOST`.

Text: D1 says "three returning routes, plus one named exception"; the site comment, the ADR passage
and the runner comment never say "every route". The risks section carries it.

### R2. The guard must strip `//` comments before masking literals.

`run()` holds ten comment lines with apostrophes (`` `run`'s ``, `loop's`, `winit's own`, `'s`);
a char-literal masker that runs first swallows from the first apostrophe to the next and takes the
`poll_events` closure's braces with it, so the loop close is either "could not locate" (a spurious
red the builder would then weaken the masker to fix) or mis-located. Strip `//` to end of line
first, then string and char literals, then walk braces. Add to the red-mutant list: delete the code
line and leave `window.callbacks().clear()` in a comment (red).

### R3. Tighten the guard's shape tests and guard the region against inversion.

- Test 2 (take precedes clear) is green on `if let Some(w) = platform.window.lock().take() {
  w.callbacks().clear(); }`, the exact shape D2/D5 forbid (the scrutinee's `MutexGuard` lives
  through the body). Require: the take line, trimmed, starts with `let ` and ends with `;`; the
  clear line contains no `.lock()`; the clear's receiver identifier equals the take's binding.
- If a masking defect shifts the loop's close upward, the region swallows the `Destroy` arm and the
  guard degrades to a file-scoped `contains`, the inversion a grep oracle is known for. Assert the
  region contains no `MainEvent::`, no `=>`, no `poll_events`, and bound its length.
- Not catchable textually, and the file header says so: a conditional wrapper that keeps the line
  and loses routes. "Present in the region", never "reached".

### R4. Evidence claims corrected.

- The surface mutant (drop `on_surface_status_change` from `clear_now`'s take-all) is already red
  under `surface_status_change_cleared_from_inside_is_not_resurrected`; the frame mutant is the
  first real pin (the one existing `is_none()` assertion on that slot is vacuous, the slot is never
  registered there). The mutant only a `Weak` probe discriminates from slot-emptiness is
  `std::mem::forget(dropped)` in `clear_now`; name it as the third mutant. Correct "the only
  capture-release pin in the workspace is winit's": `surface_lease.rs`'s
  `dropping_the_lease_drops_the_only_strong_target_ref` is a `Weak` probe on the next link.
- D5's "no `Drop` reaches a platform mutex" cited an empty grep. `RasterOwner` (a field of
  `RasterLane`) has a `Drop` (`raster_owner.rs:1616`) that locks the mailbox and runs
  `notify_retired`, which calls the mailbox wake hook. The conclusion survives because
  `set_wake_hook` has no caller outside that file's tests, so the hook is `None` on Android and the
  lock is the mailbox's own; cite that.

### R5. One more text site, and D2's unstated assumption.

- `platforms/android/window.rs:168–171`, the SAFETY comment: "nothing clears that field on a
  termination, so `self` — and the borrow — outlives the pointer." After D2 the exit path is a second
  writer of the field. The SAFETY argument stands (the pointer's bound is the `ANativeWindow`
  refcount; no `window_handle` call follows the loop), so amend the sentence to say the field is
  taken once, at the loop's exit, after the last dispatch, and keep the conclusion. It is the one
  other place in this backend asserting the field is never cleared, in the claim family this
  backend's defects have come from.
- D2 assumes one window. A second `OwnerPlatform::open_window` during the loop overwrites the field
  without clearing the displaced window's callbacks, so the exit path clears only the latest
  window. Pre-existing and not worsened; state it as the assumption, not an invariant.
- ADR-0063 decision 5's `platforms/{macos,windows,winit,headless} all do` list sits in the census
  bullet; passage 1 covers both. And the ADR amendment says the Android site is interim under the
  forward shape ALT-2 names (the lane owned by the realm slot, dropped by `teardown_platform_realm`
  on every returning route, with window closures holding a handle), which ADR-0045/#559 already
  point at; out of scope here.

## Confirmed by both lenses, no change

No third strong holder of the lane; the quit-route surface drop is reached through the clear on the
loop thread with the `ANativeWindow` live; `AndroidWindow` and `WindowCallbacks` have no `Drop`;
`invoke_quit` runs after the `window` guard is released; no second registrant on the Android window
(`open_secondary_window` is `cfg(not(android))`); recreation is a new thread/`AndroidApp`/platform;
`on_close` on the quit route is honestly unfired, as on winit; after `android_main` returns the glue
sets `thread_state = Stopped`, so the JVM thread's `set_window(None)` park falls through.

## Out of scope, recorded

ALT-1 (the `Drop` guard) with its cost; ALT-2 (presentation-owned lane) as the forward shape;
`active_window()` answering `WindowId(0)` while `AndroidWindow::id` is `WindowId(1)`; the
`runtime-contract.toml` `run_app_android` entry that still says `on_ready` fires at the first
`Resume`.
