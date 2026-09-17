# Acceptance — issue #1187: Android has no `callbacks().clear()` site

Spec: plan.md

Branch `vanyastaff/1187-android-callbacks-clear-site` from `98665a47`. Review mode: full (a platform
lifecycle decision on a path no gate executes, touching `flui-platform` and `flui-app`).

## Why it matters

A window's callback slots pin the raster lane, which pins the renderer, which pins the window: a
strong cycle whose only breaker is `WindowCallbacks::clear()`. Every other windowed backend clears
on its window-destroy path (Win32 and AppKit since #1145, winit in `finish_shutdown`, headless in
its close path). Android had no site, so the cycle is never broken there: each activity recreation
strands one window, lane and renderer, and the runner's registration is idempotent only by absence.

## Gates

A1 is runnable: its three pins are a host test, a source guard and an Android type-check, all of
which a command decides. A2 to A5 are manual by construction, not by convenience: every one of them
is a claim about a path that no host this suite runs on can execute (`AndroidPlatform::run` needs a
live `AndroidApp`), and their pins in the plan are reads. Each EVIDENCE below names what was read,
where, and by whom.

A runnable gate's `EVIDENCE:` is written by the studio's acceptance checker, in the shape
`rs-acceptance/v1 def=… exit=… expect=… out=… cwd=… shell=… at=…`: `def` is a digest of that gate's
own `CHECK`/`EXPECT`/`CWD` (editing any of them makes the gate stale until it passes again), `exit`
and `expect` are the run's outcome, `out` fingerprints the output, `at` is when it ran. A manual
gate's `EVIDENCE:` is human text instead, as above.

- [x] A1: the cycle is broken at the loop's exit on every returning route — the callback slots are cleared, and the platform's own window reference is released before them
  CHECK: env CC_aarch64-linux-android=clang AR_aarch64-linux-android=ar cargo check -p flui-app --locked --target aarch64-linux-android > /tmp/a1-android-check.log 2>&1 && FLUI_HEADLESS=1 cargo nextest run -p flui-platform --all-features -E 'test(android_run_) or test(clear_after_top_level_dispatches)'
  EXPECT: /3 tests run: 3 passed/
  CWD: .
  EVIDENCE: rs-acceptance/v1 def=d92acce7a48d8f77 exit=0 expect=matched out=2e173c995c7a0b23:783 cwd=. shell=sh at=2026-09-17T03:22:59.520Z

- [x] A2: a recreated activity re-registers on a fresh window rather than on the cleared set
  EVIDENCE: Read 2026-09-16 in `android-activity` 0.6.1's registry source and in this tree, by the plan, by `rust-reviewer` and by the orchestrator: `native_activity/glue.rs`'s `ANativeActivity_onCreate` spawns exactly one thread per activity instance running `rust_glue_entry`, which builds a fresh `AndroidApp` and calls `android_main` again; `platforms/android/mod.rs`'s `open_window` constructs `Arc::new(AndroidWindow::new(..))`, so a new platform carries a new window with fresh `WindowCallbacks`; and `runner/android.rs`'s `bootstrap_android` runs only from `on_ready`, which `run` `take()`s exactly once (`PlatformReadyCallback` is `FnOnce`). Nothing in `run` or `run_android` registers after the clear, so the once-then-discarded rule `WindowCallbacks::clear` documents is unreachable from this runner.

- [x] A3: nothing dispatches to the window after the clear
  EVIDENCE: Read 2026-09-16 by the plan, confirmed by `rust-reviewer`: the `window` field is `take()`n before the clear, so no later arm and no tail can find a window to dispatch to; `run`'s tail touches `handlers` and `bootstrap_error` only; `invoke_quit` reaches the runner's quit hook through the `Copy` `realm_dispatch` address, not a callback slot; `realm_dispatch.rs`'s `teardown_platform_realm` removes realms and clears the clipboard and redraw slots without dispatching to the window. `active_window()` answers `None` after exit and has no caller outside `flui-platform`.

- [x] A4: the clear runs synchronously at the top level of the window's FIFO, outside the platform's `window` lock
  EVIDENCE: Read 2026-09-16: the site sits after `poll_events` has returned and after the loop's own input and frame dispatches, so no `CallbackLease` is live and `DispatchDrain::begin` finds `dispatching == false`, running `clear_now` directly rather than queueing behind a drain. The lock half is mechanical: `crates/flui-platform/tests/android_exit_path.rs`'s `android_run_clears_the_window_callbacks_between_its_loop_and_the_quit_hook` asserts the clear line carries no `.lock()`, that the take is its own `let` statement, and that the clear's receiver is the take's binding — which refuses the fused `if let Some(w) = platform.window.lock().take() { w.callbacks().clear(); }` whose scrutinee guard would live through the clear (edition 2024). Both refusals were shown red by `rust-reviewer` against copies of the real files.

- [x] A5: the runner's registration comment describes the mechanism that now exists
  EVIDENCE: Read 2026-09-16 by the orchestrator and swept by `rust-reviewer`: `crates/flui-app/src/app/runner/android.rs` step 8b's two bullets now state the exit-path clear, its three returning routes plus the named panic exception, that the clear drops the two closures owning `lane`, and why nothing can register on the cleared set afterwards. The sweep for the retired claim family (`idempotent-by-absence`, `nothing clears`, `still closed`, `Filed as a follow-up`, `every route`) found no survivor in shipped code or docs outside the ADR's own dated amendment notes, which quote the old wording deliberately.

## Explicitly not in this change

The same-window recreate panic (#1185); the Android example crates (#1188); the `flui-app`
cross-typecheck line (#1189, parked). Recorded as named and unabsorbed in `review-ledger.md`: a
panic that unwinds out of `run` skips the exit region, by decision; two pre-existing locking
hazards in `run` (`invoke_quit` under the `handlers` lock, every arm dispatching under the
`window` lock); and `docs/runtime-contract.toml`'s `run_app_android` entry, which still says
`on_ready` fires at the first `Resume` where it fires at the first `InitWindow` since #1186 —
not claimed as filed.
