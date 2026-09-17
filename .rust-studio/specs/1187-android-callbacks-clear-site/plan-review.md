# Plan review brief — issue #1187

Read-only. Attack the plan, not code; nothing has been written. Do not edit, commit, stash or
revert. You may run cargo/rg commands that leave the tree unchanged; no `just ci`.

Worktree: `/home/vanyastaff/orca/workspaces/flui/android-drop-the-wgpu-surface-on-paused-terminat`,
branch `vanyastaff/1187-android-callbacks-clear-site`, HEAD `98665a47`, clean. Run everything from
there. The tree is frozen while you read.

Read in order: `gh issue view 1187`; then in `.rust-studio/specs/1187-android-callbacks-clear-site/`
`acceptance.md` (A1–A5), `scout.md` (edit-site map), `plan.md` (the plan under attack, 484 lines,
decisions D1–D5, verdict ACCEPTABLE).

## What the plan proposes

On Android, break the callback cycle (`window.callbacks → on_request_frame / on_surface_status_change
→ Arc<RasterLane> → Renderer → Arc<dyn WindowTarget>`) on **the exit path of `AndroidPlatform::run`**,
after the `loop` and before `invoke_quit()`: `take()` the platform's `window` field, then
`callbacks().clear()`. Not in the `MainEvent::Destroy` arm, because returning from `android_main` is
itself how an activity finishes (`rust_glue_entry` calls `ANativeActivity_finish` right after), so a
`quit()` or a bootstrap-error exit never sees `Destroy`. Pinned by: a capture-release test on
`WindowCallbacks` with two `Weak` probes; a function-scoped source guard test locating the exit
region of `run` and failing explicitly when it cannot; both Android type-checks. Plus the runner's
step-8b comment, `clear()`'s call-site doc, and three ADR-0063 passages.

## Attack these

1. **D1's three-route claim.** The plan says `run`'s loop has exactly one exit and three routes into
   it. Find a fourth (a panic inside `poll_events`? an early `return`? the `on_ready` `Err` path's
   `continue`, which the plan says loops back to the `break`: verify). For each route, does the clear
   run, and does anything on that route still need a slot after the clear?
2. **The quit-route surface drop.** On `quit()` no `TerminateWindow` was delivered, so the clear
   drops a configured `wgpu::Surface` while the `ANativeWindow` is live. The plan calls that the #713
   order and accepts the `vkDeviceWaitIdle` cost. Is the drop actually reached through the clear (the
   lane is captured by two slots; does dropping both slots drop the lane, or does a third holder
   survive: the platform's `window` field is taken, but what about `APP_RUNTIME`, a queued
   `RealmTask`, `install_surface_applier`)? If a third strong holder exists the cycle is not broken
   by this change at all.
3. **`clear()` outside the `window` mutex, D2/D5.** The plan takes the field into a local and clears
   outside the guard. Trace the drop of the last `Arc<AndroidWindow>`: does `AndroidWindow`'s `Drop`
   (or `WindowCallbacks`'s) touch any platform mutex that could still be held on this path? Does the
   `take()` happen before or after `invoke_quit`, and does the quit hook ever read the field?
4. **The source guard, D3.** Read the guard's specified shape in the plan (`tests/android_exit_path.rs`:
   mask literals, locate `fn run(self: Box<Self>` exactly once, region between the depth-0 `loop`'s
   close and the `invoke_quit()` line). Construct an edit that keeps the guard green while removing
   the effect (a `clear()` on a *different* object in the region; a `clear()` inside an `if false`;
   the two statements present but in the wrong order). Say whether the guard as specified catches
   each, and if not, whether that is acceptable for a textual guard or needs tightening. Also: a
   grep-shaped oracle can invert; does this one have a vacuous-pass path?
5. **The capture-release test.** It is green on HEAD by design (a discrimination proof). Is it
   actually discriminating: name the production line whose removal fails each `Weak` probe, and
   check whether the existing six `handlers.rs` tests already cover one of the two mutants the plan
   names, making one probe redundant.
6. **D4, same-object re-registration.** The plan says a second `bootstrap_android` in one
   `android_main` is unreachable because `on_ready` is `FnOnce`. Find another registrant on the
   Android window after the clear: `open_secondary_window`? a hot-reload re-bootstrap? the
   `on_surface_status_change` re-registration the runner does on … anything? If one exists it runs
   once then is discarded, silently.
7. **`on_close` on the quit route.** The plan names it as a cross-backend contract question and
   leaves it. Is that honest, or does this change make it *worse* on Android (the field is taken
   and cleared, so a late `on_close` can never fire), turning an unfired callback into an
   unfireable one?
8. **Text sites.** `handlers.rs`'s `clear()` doc enumerates call sites by hand; the ADR has three
   passages. Is there a fourth place that says Android has no site (`crates/flui-platform/ARCHITECTURE.md`,
   `AGENTS.md`, the runner's module doc, `docs/runtime-contract.toml`)? `rg -n "no.*clear\|never clears\|nothing clears" --glob '*.md' --glob '*.rs' --glob '*.toml' crates docs` is a start.

## Verdict

`ACCEPTABLE`, `RESHAPE NEEDED` or `BLOCKED`, numbered findings tagged blocking/advisory, cited by
file and symbol. No praise. `unverified` where you cannot settle something, naming what would. If the
plan is right, one line and stop. A `Blocking waiting for file lock` line is another session's cargo.
