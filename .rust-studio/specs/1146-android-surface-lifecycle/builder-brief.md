# Builder brief — implement issue #1146

You are implementing an approved plan. The plan is the specification; this brief adds only the
working rules, the evidence you owe, and the constraints that are easy to miss.

## Read first

- **The plan: `.rust-studio/specs/1146-android-surface-lifecycle/plan.md`** (about 1100 lines). Read
  it in full. Every acceptance criterion (A1 to A9) carries a `*Pin:*` line naming the evidence it
  requires, and the edit-site map (14 rows) names every file to touch.
- `survey.md` in the same directory: the market and dependency evidence behind decisions B, L and the
  ADR's citations. You do not need to re-derive it, and you must not contradict it without evidence.
- `review-brief.md` opens with a note saying which names it supersedes. It is the historical artifact.

## Repository and write zone

- Work in `/home/vanyastaff/orca/workspaces/flui/android-drop-the-wgpu-surface-on-paused-terminat`
  (worktree, branch `vanyastaff/android-drop-the-wgpu-surface-on-paused-terminat`, HEAD `4aa8782b`).
  Never `cd` to the main checkout at `/mnt/data/dev/flui`.
- **Write only the files the edit-site map names.** The `.rust-studio/specs/1146-android-surface-lifecycle/`
  directory is a read-only input; do not edit the plan, the survey, or this brief.
- **Do not commit, do not stash, do not push.** Leave the change in the working tree so the
  orchestrator can review `git diff`. Never use bare `git stash` in this worktree.
- Run `git branch --show-current` before your first write and confirm the branch above. Re-check it
  before any `git` command that could touch another branch.
- Add files with explicit paths (`git add <path>`), never `git add -A`.

## Required evidence

**Red first, before the fix, for every behavior change, with the failing output quoted in your
report.** Two forms are acceptable and must be labelled for what they are:

- **Mutation-red** for the seam, lease and callback tests: get them green, then invert the production
  arm each test names, show the test fail, restore it. Required, not optional.
- **Compile-red** only for the additive API pieces, where a test naming `release_surface`,
  `recreate_surface` or `SurfaceAcquireOutcome::Released` cannot build against the unmodified tree.
  Label it compile-red; a compile error is weak evidence and must not be dressed as behavioral.
- A9 is genuinely red on the unmodified tree: `cargo check` reports one `E0308`. Quote it.

**Never fake a pass.** No stub, no `Size::ZERO`, no narrowing a test to the part that already works,
no `assert!(result.is_ok())` where the value is the claim. If you did not implement something, say so
rather than adjusting a test until it passes.

## Gates, in this order

1. `just fmt-check`
2. `just clippy`
3. focused suites first: `cargo nextest run -p flui-engine`, `cargo nextest run -p flui-app`, and for
   `flui-platform` the CI device: `FLUI_HEADLESS=1 cargo nextest run -p flui-platform --all-features`
   (nine winit-internals tests need an X11 connection; `just test-ci` wraps the CI step and needs
   `xvfb-run`).
4. `just ci` (fmt-check → inventory-check → runtime-conformance-check → port-check → clippy → test →
   test-doc). This is the repository's gate; `just clippy` and `cargo nextest` alone are not.
5. **Last, and separately: the Android check**, which is the only thing that sees rows 6, 7 and 12:

   ```
   env CC_aarch64-linux-android=clang AR_aarch64-linux-android=ar \
       CFLAGS_aarch64-linux-android="--target=aarch64-linux-android" \
       cargo check -p flui-app --locked --target aarch64-linux-android
   ```

   It takes about 24 s wall here. `just cross-typecheck` is the repository's own Android check and
   covers `flui-platform` only; run it too.

## Constraints (binding)

- `tracing` only in shipped code: no `println!`, `eprintln!` or `dbg!`.
- `thiserror` in libraries, `anyhow` in binaries; `expect("BUG: <invariant>")` for internal
  invariants; no bare `unwrap()` on production paths (`clippy::unwrap_used` gates it).
- **No process-ID markers** in code, comments, doc comments, file names or test names: no `Cycle N`,
  no `PR #NNN`, no agent-pass ids, no bare `U##`, no `SC-NNN`. State the invariant in plain English.
  `ADR-NNNN` and `FR-NNN` references are fine; they are grepped by checkers.
- Every new public item gets a doc comment, with `# Errors` / `# Panics` sections where they apply.
- Match the surrounding idiom: the sibling seams are `runner/device_recovery.rs` and
  `runner/lifecycle_ladder.rs`, and `build_windowed_gpu_stack` in `renderer.rs`.
- No compatibility shims and no half-migrations. This is active development; reshape what the plan
  says to reshape.
- `unsafe`: this change adds none. If you find yourself needing one, stop and report instead.

## Settled decisions, not open for relitigation

The plan's "Design decisions" section (A to L) is the record. In one line each, the ones a builder
most often wants to change:

1. Release on `Pause` and `TerminateWindow`; recreate on `Resume` and `InitWindow`. Unconditional
   recreate, never "skip if a surface is already held" (decision J).
2. Surface only. Instance, adapter, device and queue are never rebuilt (decision C).
3. A per-window callback, not a polled query, and not `on_active_status_change` (decisions A, B).
4. `release_surface` / `recreate_surface` in `renderer.rs`, never "reacquire" (decision K).
5. `SurfaceAcquireOutcome::Released` stays distinct from the not-windowed `SurfaceLost` (A2).
6. No `GpuStackOrigin::is_windowed()` helper, and no counter: the consumer is a log line (A8).
7. The callback takes the lane with a **blocking** lock, and its call-site comment names the
   invariant "nothing off-thread ever holds the lane" plus the class of change that breaks it
   (decision G).
8. `on_ready` fires at the first `MainEvent::InitWindow`, and the `MainEvent::Destroy` arm records a
   `BootstrapError` when `on_ready` is still untaken (A2b).

## When the plan and the tree disagree

The tree wins, and you report the disagreement. Two places the plan already flags as reading-only
(A2b's arm placement and row 12's registration) are exactly where a surprise is likeliest. Report it
rather than silently choosing.

## Report back

- `git diff --stat`.
- Per acceptance criterion (A1 to A9): the pin's command and its result.
- The red-first evidence for each behavior change: the command and the failing output.
- Gate output tails (clippy, `just ci`, `just cross-typecheck`, the Android check).
- Anything in the plan you did not implement, anything you found wrong in it, anything you could not
  verify, and anything you changed that the edit-site map does not name.
- Verdict: `COMPLETE`, `NEEDS WORK`, or `BLOCKED`.
