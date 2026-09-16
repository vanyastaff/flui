# Repair re-review — issue #1146

Read-only. Do not edit, commit, revert, or stash. Restore the tree exactly if you mutate anything.
Another session may be running cargo on this machine, so a `Blocking waiting for file lock` line is
the shared package cache and must be reported as such rather than attributed to this change.

Worktree: `/home/vanyastaff/orca/workspaces/flui/android-drop-the-wgpu-surface-on-paused-terminat`
(branch `vanyastaff/android-drop-the-wgpu-surface-on-paused-terminat`, HEAD `4aa8782b`). Run every
command from there. The change is `git diff` (16 files) plus one untracked file,
`crates/flui-app/src/app/runner/surface_lifecycle.rs`.

The findings that produced this pass are in `repair-1.md`; the rulings behind them are in
`review-ledger.md`, both in `.rust-studio/specs/1146-android-surface-lifecycle/`. Read them for
context, then judge the tree, not the paperwork.

**The tree is frozen.** No agent is editing it while you read. `git diff HEAD | sha1sum` is
`aee038fc2603457edc75f08e39545b6f181dd5a0` at the moment this brief was written; run it first, and if it differs, stop and report the
drift rather than reviewing, because a mismatch means something else wrote to the tree.

Two repair passes have landed. Pass 1 covered the blocking findings B1 to B3 and advisory A1 to A9
(`repair-1.md`); pass 2 covered six text-only corrections R1 to R6 from the outside lens
(`repair-2.md`). Both are claims until you check them. Pass 1's gates were reproduced against the
tree as it stood after pass 1 (`just ci` EXIT=0 with 10271 tests, the Android check EXIT=0,
`cross-typecheck` EXIT=0 on three targets); pass 2 changed only doc comments, one test name and two
Markdown files, and `just ci` is being re-run on this frozen revision concurrently with your read.
Do not spend your budget re-running the gates; spend it on the code.

## The question for every lens

**Did the repair introduce a regression, and is the new code correct?** One item added production
code; the rest are documentation, tests, and scope. Review the *repair*, not the whole change again.

## B1 — the one new code path (highest risk, no lens has seen it)

`crates/flui-engine/src/wgpu/renderer.rs`, `Renderer::recreate_surface`. It now reads
`pipelines_format` off `self.painter` (`WgpuPainter::surface_format`), and when that differs from the
freshly derived `fresh_config.format` it rebuilds **both** `self.painter` (a new `WgpuPainter`) and
`self.offscreen` (a new `OffscreenRenderer`) from the fresh format, between `surface.configure` and
`lease.replace_surface`.

The defect it closes is real and was verified before the repair: the format is baked into every
pipeline and into the offscreen pool's textures at construction, while the per-frame target format is
read from `self.config`, and `on_uncaptured_error` only logs, so a moved format blanked the window
with one `error!` per frame and no self-heal.

`recover` rebuilds the same two fields, but only as part of replacing the *entire* GPU stack
(instance, adapter, device, queue), which is a far more disruptive act, and it does so on a path that
already assumes everything downstream is being thrown away. `recreate_surface` does it mid-life, on a
lifecycle callback, keeping the same device and queue. Attack that difference:

- **What else holds state derived from the old painter or offscreen renderer?** A pooled texture, a
  cached bind group, a cached offscreen painter, a cached `PipelineSet`, a layout, a sampler, a
  profiler entry. `renderer.rs` has `get_or_create_offscreen_painter` call sites; find where those
  caches live and whether replacing `self.offscreen` invalidates them or strands them. A rebuild that
  leaves a cache pointing at pipelines built for the old format reintroduces the same defect one
  layer down, and the gate would stay green because this path needs a GPU to execute.
- **Is the ordering sound for the fields it does not touch?** `self.config`, `self.supports_copy_src`
  and the damage tracker are committed after `replace_surface`. Trace what a frame arriving between
  the rebuild and the commit would read.
- **Is `pipelines_format != Some(fresh_config.format)` the right condition?** Check the `None` case
  and whether any state can make the painter's format and `self.config.format` legitimately disagree
  before this point, in which case the guard fires on a resume that changed nothing.
- **Does it allocate on a path that was documented as cheap?** `recreate_surface`'s own doc and
  `derive_surface_config`'s note discuss cost; the `release_surface`/`recreate_surface` contract
  deliberately avoids waiting on a raster lane or submitting anything. Say whether an unconditional
  pipeline rebuild under a moved format is acceptable there, and whether it is bounded.

## The other repaired items, by lens

- **`unsafe-auditor`:** `crates/flui-platform/src/platforms/android/window.rs`, the `// SAFETY:` block
  on `AndroidWindow::window_handle`. This was your blocking finding and the sole reason you withheld
  SAFETY-GATE. The borrow-lifetime clause is deleted; the block now states the bound on the pointer
  (the `ANativeWindow` refcount), says explicitly that the returned `'_` is not that bound, keeps the
  prohibition, and names the obligation as consumer-enforced with its discharger. **Verify the
  dependency facts yourself** (`ndk`'s `Clone`/`Drop`, `android-activity` 0.6.1's `pre_exec_cmd`
  / `post_exec_cmd` and `set_window`) rather than trusting the new comment, then sign or withhold
  SAFETY-GATE. Also check `traits/window.rs`'s MUST doc (your finding 2) now separates our stricter
  MUST from upstream's actual wording instead of restating the false premise.
- **`rust-reviewer`:** the rest of the repair for correctness regressions and test quality.
  `raster_lane.rs`'s doc and log (B2, the second blocking finding); the narrowed lane guard in
  `crates/flui-app/src/app/runner/android.rs` (A5, the only other code change — the guard now ends
  after the mint and the realm dispatch runs outside it); the lease test's hoisted fixtures and its
  changed assertion (A6); and the text corrections in `raster.rs`, `error.rs`, `traits/window.rs`,
  `platforms/android/window.rs`, `renderer.rs`'s `derive_surface_config` note, and ADR-0063.
  For A6, judge whether the new assertion can actually fail and whether the builder's stated limit
  (that it is not itself the aborting failure under the mutation) is the honest description.
  For A7, judge the three ADR corrections on the merits, especially whether the two `unverified`
  markers are attached to the right claims and whether anything asserted there is now false.

## Verdict

`ACCEPTABLE`, `NEEDS WORK`, or `BLOCKED`, with concrete findings tagged **blocking** or **advisory**
and cited by file and symbol. Say `unverified` where you cannot settle something, and name what would.
No praise. If the repair is correct, say so in one line and stop.
