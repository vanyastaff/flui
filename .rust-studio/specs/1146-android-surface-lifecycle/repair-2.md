# Repair pass 2 — findings from the outside lens

Repair pass 1 is verified: B1's guard reads the consumer, B2's text is cause-neutral, B3's SAFETY block
states the pointer's bound, A5's guard ends at the mint, A6's fixtures are hoisted, and the gates
reproduce independently (`just ci` EXIT=0 with 10271 tests, the Android check EXIT=0, `cross-typecheck`
EXIT=0 on three targets).

This pass fixes the outside lens's findings. **Every item is text. Do not change behaviour, and do not
reopen anything pass 1 settled.** The two items that touch a test and an architecture doc are still
text-only: a rename and a citation.

Constraints as before: same worktree, no commits, no stash, no push, `git add` explicit paths only,
and nothing under `.rust-studio/specs/` edited. **Do not start any work until this pass, and do not
edit any file this brief does not name** — a review ran against the tree while you were editing it
last time, which wasted that lens's gate work. Keep the edit set to exactly the files below.

## What the lens found

The three items below are claims the repair, or the change, made that are not true of the code.

### R1. The macro mitigation is a convention stated as a mechanism

`crates/flui-platform/src/traits/window.rs`, the sentence A9 added saying every real window's setters
come from `impl_window_callback_setters!` so "a backend cannot override this method without storing
its callback — the whole family moves together".

That is false for anyone outside `flui-platform`. The macro is `pub(crate)`
(`shared/mod.rs`, `pub(crate) use handlers::impl_window_callback_setters`), so no implementor in
another crate can use it at all, and `flui-app`'s `TestWindow`
(`crates/flui-app/src/app/window_test_support.rs`, `impl PlatformWindow for TestWindow`) is a live
counterexample: it overrides no setter. The paragraph two sentences above already admits the case this
sentence excludes, so the two contradict each other in the same block.

Fix: state it as a convention scoped to the backends that can use the macro (the in-crate ones), and
say plainly that an out-of-crate implementor is not covered and must store the callback itself. Keep
the hazard's description; drop the guarantee. My instruction that produced this sentence said "in-crate
mitigation", so the scoping was lost in the writing, not in the intent.

### R2. The `Failed` doc mandates a level its only caller overrides without saying so

`crates/flui-app/src/app/runner/surface_lifecycle.rs`, `SurfaceLifecycleOutcome::Failed`'s doc: "log
the carried error as the `source` of a `warn`". The caller
(`crates/flui-app/src/app/runner/android.rs`, the `Failed` arm) logs `SurfaceTargetUnavailable` at
`trace!` instead, and justifies that in its own comment because that classification occurs on every
cycle by design. One of the two is wrong, and the contract doc loses.

Fix: name the exception in the seam's doc. `SurfaceTargetUnavailable` is the expected answer to the
acquire half and is the caller's to downgrade; every other error is the `warn`.

### R3. The failed-recreate residual is unnamed, and the seam reads as if recovery is automatic

Same `Failed` doc, and the caller's `Failed` arm. A recreation that fails for a non-probe reason leaves
the presentation released, nothing re-asks until another `true` arrives, and the only `true` emitters
on this backend are `Resume` and `InitWindow`. So the app stays blank until a lifecycle event, which is
the same visible symptom the rest of this change exists to remove. The device-loss half is covered: a
lost device sets the flag, so the sibling recovery path retries it under its backoff and wakes the
loop. The uncovered half is a non-device-loss `SurfaceCreation` or validation failure.

This is a real residual, and it is **not** to be fixed by adding a retry: the plan declines a retry
loop deliberately, and a retry on this backend means re-arming the wake hook and a backoff, which is a
mechanism with its own evidence requirements. Fix the text instead, and be precise about three things:

- the residual is named where the caller's arm decides not to retry, and where the seam's `Failed` doc
  claims the caller's obligation is discharged by logging;
- why no retry: the probe must succeed at `InitWindow` (the window is set before the callback runs), so
  a failure there is genuine rather than a timing race, and a poll is the wrong answer to a genuine
  failure;
- what does cover the failure class that can self-heal (the device-loss path, by name), so a reader is
  not left thinking the seam's statelessness makes every failure recover itself. It does not: it makes
  a *missed signal* recoverable, which is a different claim, and the file should not let the two blur.

Record it as a follow-up in the report rather than implementing it.

## Four smaller text corrections

### R4. The lease test's name claims an order its state no longer has

`crates/flui-engine/src/wgpu/surface_lease.rs`, `a_released_lease_still_drops_its_target_last`. After
`release()` the surface is gone, so the target cannot be dropped "last" of anything; the body and its
comment are now honest about this, and only the name overstates. Rename it to what it now asserts (a
released lease's drop releases the target and nothing else). Keep the body exactly as pass 1 left it.

### R5. The release's unbounded wait runs under the held lane, and nothing says so

`crates/flui-app/src/app/runner/android.rs`, the callback's comment block, and
`surface_lifecycle.rs`'s `release_surface` doc. The seam's doc says the verb "must not wait on a raster
lane", and its only caller takes the **blocking** lane guard and then calls it; for `false` that call
drops a configured surface, which is the unbounded `vkDeviceWaitIdle` the seam's own doc names as the
cost. So the wait the doc is careful about happens with the lane held, on the Android main thread,
which must return before `post_exec_cmd(AppCmd::TermWindow)` clears the handle.

The design is deliberate (the blocking lock is what makes the release complete before the callback
returns) and this is not a request to change it. What the text must stop doing is reading as if the
release were cheap and lane-free: say that the wait runs under the held guard by design, and why that
is accepted. Name the Android input-dispatch watchdog as the contingent risk and mark it unverified,
since the wait's length is the driver's and no device is available here.

### R6. Two cost-accounting and citation corrections in the ADR and ARCHITECTURE

- `docs/adr/ADR-0063-the-renderer-owns-its-surface-target.md`, the trade-off paragraph that books the
  cost as "per cycle, not per defect" and describes a cycle as one release, one failed probe, one
  rebuild, one mint and one repaint. Both `Resume` and `InitWindow` emit `true`, and the seam recreates
  unconditionally on `true` by design, so on the ordering where `InitWindow` precedes `Resume` a single
  activity recreation pays two create/configure pairs, two mints and two full repaints. Correct the
  accounting to per edge and name the ordering that pays twice. Keep the statelessness rationale; only
  the count and the unit are wrong.
- `crates/flui-platform/ARCHITECTURE.md`: the sentence this change added cites the Flutter API the
  issue *asked* for (`onSurfaceDestroying`) and never names the one that landed
  (`SurfaceProducer.onSurfaceCleanup`). ADR-0063 names both. Name both here too, and make clear which
  of the two exists, so a reader grepping the cited spelling is not left thinking the citation is
  wrong. Two lenses flagged this and the one that rejected it did so on a premise this refutes (the
  landed name does appear in the repository, in the ADR).

## Do not implement

- The retry mechanism for a failed recreate (R3 records it, the plan declines it).
- Any change to the blocking-lane decision itself, including moving the release outside the guard
  (R5 is a wording fix).
- Anything about the review's own artefacts. The lens's C1 to C3 note that it reviewed the tree while
  this change was being edited, and that the stat in the review brief was the pre-repair one. Those are
  my errors of process, not defects in the tree, and they need no code.

## Report back

Per item: what you changed and in which file. Quote the final `git diff --stat`. Re-run `just ci` and
the Android check (`env CC_aarch64-linux-android=clang AR_aarch64-linux-android=ar
CFLAGS_aarch64-linux-android="--target=aarch64-linux-android" cargo check -p flui-app --locked --target
aarch64-linux-android`), since a rename touches a test name and the docs are compiled by `doc-strict`.
Say explicitly if you did not implement an item. Another session may hold the package cache, so report
a `Blocking waiting for file lock` line as shared-cache contention rather than a property of this
change.
