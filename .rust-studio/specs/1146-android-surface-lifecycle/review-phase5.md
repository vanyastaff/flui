# Review brief — issue #1146, working-tree diff

You are reviewing an uncommitted change. Read-only: do not edit, commit, or revert anything. The
repository's own gates are the ones to run; leave the tree exactly as you found it.

## Where

- Worktree: `/home/vanyastaff/orca/workspaces/flui/android-drop-the-wgpu-surface-on-paused-terminat`
  (branch `vanyastaff/android-drop-the-wgpu-surface-on-paused-terminat`, HEAD `4aa8782b`).
  Run every command from there; never `cd` to the main checkout.
- The change: `git diff` (14 modified files) plus one untracked file,
  `crates/flui-app/src/app/runner/surface_lifecycle.rs`. `git diff --stat` is
  `14 files changed, 1215 insertions(+), 147 deletions(-)`.
- The specification: `.rust-studio/specs/1146-android-surface-lifecycle/plan.md` (about 1100 lines),
  with `survey.md` and `builder-brief.md` beside it. The plan is authoritative. `review-brief.md` in
  that directory is a historical artifact from an earlier plan-attack and is superseded.

## What the change is

Android's `wgpu::Surface` survived a pause, because nothing in the Android backend handled
`MainEvent::TerminateWindow`/`InitWindow` and the only path that rebuilds a surface is device-loss
recovery, which a suspend never triggers. The change makes `SurfaceLease::surface` optional, adds
`Renderer::release_surface`/`recreate_surface` (surface-only, keeping instance/adapter/device), adds
`SurfaceAcquireOutcome::Released` so a released renderer skips a present instead of reporting
`SurfaceLost`, adds a per-window surface-lifecycle callback that the Android backend fires on
`Pause`/`TerminateWindow` (false) and `Resume`/`InitWindow` (true), wires it through a new
`pub(super)` seam in `flui-app`'s Android runner, corrects stale documents, and fixes a pre-existing
`E0308` that stopped the Android runner compiling at all.

## The builder's report is a claim, not ground truth

The implementer reported `COMPLETE`. Everything below is its assertion; verify what you rely on:

- Gates all EXIT=0: `just ci` (third attempt), `just fmt-check`, `just clippy`, the three focused
  nextest runs, `just cross-typecheck`, and the Android check
  (`env CC_aarch64-linux-android=clang AR_aarch64-linux-android=ar CFLAGS_aarch64-linux-android="--target=aarch64-linux-android" cargo check -p flui-app --locked --target aarch64-linux-android`).
  It reports 10271 tests passing in the workspace run and 289 in the flui-platform headless run.
- Two defects it says the gate caught and it fixed: a missing `runtime-contract.toml` lock marker,
  and three broken intra-doc links under `doc-strict`.
- Red-first evidence: fresh mutation probes on the lease (`SurfaceLease::release` made a no-op → 3 of
  6 tests fail) and the callback (`SurfaceStatus(!has_surface)` → 2 of 3 fail), plus earlier probes
  on the seam, plus a compile-red for the `#[must_use]` outcome and a red→green for the `E0308`.
- **Two edits outside the plan's edit-site map**, which it reported rather than hid:
  (a) `docs/runtime-contract.toml`, one marker line added to an existing `[[lock_exemption]]` entry,
  which it says the gate forces; and (b) an eighth stale doc passage it found, in
  `crates/flui-app/src/app/runner/android.rs`'s `bootstrap_android` doc.
- One item it explicitly left out of scope: `crates/flui-app/src/app/raster_lane.rs:277-281`, where
  `RasterLane::note_surface_recreated`'s `tracing::info!` hardcodes "surface recreated by device
  recovery" and now has a second production caller, so the log text misattributes the cause.
- Honest denominators it states: the Android arms are type-checked and executed by nothing on this
  host; the `Renderer`-side mechanics are type-checked only (they need a GPU); the seam's decision
  logic is host-tested against a scripted backend.

## Stage 5a — spec compliance (this pass)

Judge the diff against the plan's acceptance criteria, A1 to A9, each of which carries a `*Pin:*`
line naming its evidence, **and** against the plan's 14-row edit-site map. For each criterion say
whether the pin is met and cite what you checked.

Answer these directly:

1. **Is anything specified but missing?** Name the criterion and the missing piece.
2. **Is anything present but not specified — scope creep?** Judge the two out-of-map edits on their
   merits: is each *forced* (the gate cannot go green without it, or the passage is made false by
   this change), or is it a builder preference? The plan itself defers a *different*
   `runtime-contract.toml` tidy; check whether the one line added is that change wearing a new hat.
3. **Does the diff contradict the plan's settled decisions (A to L)?** In particular: unconditional
   recreate (never skip when a surface is already held), surface-only release, `Released` distinct
   from `SurfaceLost`, no `GpuStackOrigin::is_windowed()` helper, and the blocking lane lock with its
   invariant comment.
4. **Are the acceptance criteria's tests able to fail?** For each new test, name the production line
   whose inversion would fail it. A test that cannot fail is not a pin. Do not mutate the tree to
   prove this in this pass — reason from the code and say when you are inferring.
5. **Does the stale-doc correction actually correct?** The plan's A6 counts seven passages; the
   builder says there were eight. Read the corrected text against the `android-activity` 0.6.1
   source in `~/.cargo/registry/src/*/android-activity-0.6.1/src/native_activity/glue.rs` and say
   whether the *corrected* claim is now true. A second wrong correction is worse than the first.
6. **Is the reported evidence reproducible?** Re-run at least `just fmt-check`, `just clippy`,
   `cargo nextest run -p flui-app`, and the Android check yourself and compare to the reported
   numbers. Re-running `just ci` is welcome and is the strongest single check. **If another agent is
   running cargo at the same time, say so in your report rather than attributing a lock wait or a
   flake to the change.**

## Verdict

`ACCEPTABLE`, `NEEDS WORK`, or `BLOCKED`, with a numbered list of concrete findings, each tagged
**blocking** or **advisory**. Cite file and symbol, not line numbers alone. No praise. If you cannot
verify something, say `unverified` and name what would settle it.

---

# Stage 5b — code quality (second pass)

Stage 5a returned `ACCEPTABLE`: A1–A9 all met, all 14 edit-map rows present, both out-of-map edits
judged *forced* rather than preference, and every gate independently reproduced (`just ci` EXIT=0
with 10271 tests, the Android check EXIT=0 with all 7 warnings in untouched files, `cross-typecheck`
green on three targets). Your pass is **code quality, not spec compliance**: correctness bugs,
soundness, standards, test quality, and whether this is the shape a maintainer would accept.

## Carry-forward from 5a — rule on these

1. **advisory** `crates/flui-engine/src/error.rs`: `EngineError::SurfaceTargetUnavailable`'s doc
   names "an Android activity between `onPause` and the next `onResume`" as a gone-handle case. This
   change established that the handle is live across a pause and dies exactly between
   `TerminateWindow` and the next `InitWindow`. The passage is not false, it is the coarser version
   of the claim this change sharpened, and A6's sweep stopped short of it.
2. **advisory, the strongest of the five** `crates/flui-app/src/app/raster_lane.rs`:
   `RasterLane::note_surface_recreated`'s doc and its `tracing::info!` hardcode the device-recovery
   cause, and the module doc's "Generation discipline" list repeats it. The plan *mandates* the
   second caller, so this change makes a previously-true log line misattribute its cause on the
   Android path. The plan's "Explicitly not in this change" list does not defer it.
3. **advisory** `crates/flui-engine/src/wgpu/surface_lease.rs`, test
   `a_released_lease_still_drops_its_target_last`: its second assertion asserts a drop order that is
   unobservable in that state, because `release()` already emptied the surface. The test fails if
   `release` stops dropping, so it is not vacuous, but its stated claim is wider than it reads.
4. **advisory, cosmetic** `crates/flui-platform/ARCHITECTURE.md`: the market-lineage sentence was
   verified true upstream (the issue asked for `onSurfaceDestroying`; the landed API is
   `SurfaceProducer.onSurfaceCleanup`). Only worth naming both spellings once so a reader who greps
   for the requested one is not left thinking the citation is wrong.
5. **advisory, routed to `unsafe-auditor`** `crates/flui-platform/src/platforms/android/window.rs`:
   the rewritten SAFETY comment's first sentence couples the `&self` borrow lifetime to the next
   `AppCmd::TermWindow`, but the same comment establishes that `AndroidWindow` is held across a
   pause, so the borrow can outlive it. The operative dereference bound is correct; only that clause
   is an incomplete justification. The change adds no `unsafe`; it edits the comment's text.

## What each lens should attack

- **`rust-reviewer` (code quality):** correctness and soundness of the whole diff, standards
  (`STYLE.md`, the binding core rules), test quality (does each new test fail if the line it names is
  inverted — reason from the code, and mutate only if you restore the tree exactly), naming, docs,
  and any hidden coupling. Rule on findings 1–3 above.
- **`api-design-lead`:** the public surface. A new defaulted trait method on `PlatformWindow`, the
  new `pub` `Renderer::release_surface`/`recreate_surface`, the new `SurfaceAcquireOutcome::Released`
  variant, the new `WindowCallbackEvent` variant, and the `#[must_use]` outcome enum. Judge semver,
  accidental `pub`, the default-body choice on an 11-implementor public trait, and whether the
  released/not-windowed disposition split is the right type-level shape.
- **`unsafe-auditor`:** the SAFETY comment in `platforms/android/window.rs` (finding 5). The change
  adds no `unsafe` block, so the question is whether the rewritten justification still soundly
  establishes `borrow_raw`'s precondition, or whether the wording now claims more than the code can
  support.
- **outside lens:** the same diff, read cold, for anything the family's shared assumptions hide.

## Constraints for every 5b lens

Read-only: do not edit, commit, revert or stash. Restore the tree exactly if you mutate anything to
test a claim. **Other agents and other worktrees run cargo concurrently** on this machine, so a
`Blocking waiting for file lock` line is the shared package cache and must be reported as such
rather than attributed to the change.
