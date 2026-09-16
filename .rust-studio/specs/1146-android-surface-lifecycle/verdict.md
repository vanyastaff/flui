# Verdict — issue #1146: drop the wgpu surface on Paused/TerminateWindow, recreate on Resume

**COMPLETE.** Both Phase 5 stages ran and returned `ACCEPTABLE`; SAFETY-GATE is signed; the
red-first evidence is on record; every gate is green on the frozen final tree, measured by the
orchestrator rather than reported. One latent hazard was found after the last review lens returned and
is named below rather than hidden.

Final tree: HEAD `4aa8782b`, uncommitted, `git diff HEAD | sha1sum` =
`8974d1ee92c1dfbda1d2b438f5c4722d65c0732d`, 16 files changed, `1577 insertions(+), 209 deletions(-)`,
plus untracked `crates/flui-app/src/app/runner/surface_lifecycle.rs`. Nothing staged, stashed or
pushed.

## What changed

Android's `wgpu::Surface` survived a pause because nothing handled `MainEvent::TerminateWindow` /
`InitWindow` and the only surface-rebuild path was device-loss recovery, which a suspend never
triggers. Now: `SurfaceLease::surface` is `Option<S>`; `Renderer::release_surface` /
`recreate_surface` are surface-only verbs that keep the instance, adapter, device and queue;
`SurfaceAcquireOutcome::Released` lets a released renderer skip a present rather than report
`SurfaceLost`; a defaulted-no-op `PlatformWindow::on_surface_status_change(bool)` is fired by the
Android backend on `Pause`/`TerminateWindow` (`false`) and `Resume`/`InitWindow` (`true`); a
`pub(super)` `SurfaceLifecycle` seam in `flui-app`'s Android runner turns the bool into an act on the
renderer, mints a raster-lane generation and marks a full repaint; `recreate_surface` rebuilds the
painter and offscreen renderer when the re-derived surface format moved; a pre-existing `E0308` that
stopped the Android runner compiling is fixed; and eleven stale passages across ADR-0063, three crate
docs and the trait docs are corrected against the pinned `android-activity` 0.6.1, `ndk` 0.9.0,
`raw-window-handle` 0.6.2, `winit` 0.30.13 and `wgpu-hal` 30.0.1 sources, plus the Vulkan
specification.

## Evidence

- `just ci`: EXIT=0, `10271 tests run: 10271 passed` + 40 + 289 (flui-platform headless), zero
  failures, doctests green.
- Android check (`cargo check -p flui-app --locked --target aarch64-linux-android` with the `cc-rs`
  env vars): EXIT=0, 7 warnings, all pre-existing and in files this change does not touch.
- `just cross-typecheck`: EXIT=0 on `x86_64-pc-windows-msvc`, `aarch64-apple-darwin`,
  `aarch64-linux-android`, zero errors.
- Red-first: mutation probes on the lease (`release` made a no-op → 3 of 6 fail), the callback
  (`SurfaceStatus(!has_surface)` → 2 of 3 fail) and the seam; a compile-red for the `#[must_use]`
  outcome; a red→green for the `E0308`.
- `cargo-semver-checks` against HEAD: 196 checks, no semver update required; no accidental `pub`.

## Review record

Stage 5a (spec compliance): `ACCEPTABLE`. Stage 5b: four lenses (`unsafe-auditor`,
`api-design-lead`, `rust-reviewer`, outside lens), two of which returned `NEEDS WORK` with three
blocking findings, all repaired and re-reviewed: `unsafe-auditor` re-signed SAFETY-GATE, and
`rust-reviewer` confirmed the one new code path is a complete invalidation. Three repair dispatches
were permitted and three were spent. The full ruling record, every finding and its disposition, is in
`review-ledger.md` beside this file.

## Named residuals (reported, not fixed)

1. **A same-window surface recreate panics under the locked `wgpu-hal`.** The Vulkan spec allows
   one `VkSurfaceKHR` per `ANativeWindow`; `recreate_surface` creates before it drops; `wgpu-hal`
   30.0.1's `create_surface_android` `expect`s the result. So a `true` that finds a held surface on
   the same window aborts the process on the callback thread instead of reaching the seam's `Failed`
   arm. Reachable on this tree only via a `false` missed on a surviving window (every arm is mapped
   and dispatches inline) or an `InitWindow`-before-`Resume` ordering the glue never emits on any
   traced path: latent behind another bug, not reachable in the ordinary lifecycle. Found by the
   builder after the last review lens returned, so no lens has ruled on it. The docs state it. The
   fix direction is drop-first order in `recreate_surface`, which revisits the plan's settled
   build-first decision and is a follow-up, not this change.
2. **B1's behavioural half is unverified.** The format-rebuild guard is structural; proving it
   needs a GPU and a surface whose capability list differs across a release/recreate.
3. **The EGL/`gles` late-drop path is unread.** Vulkan's is settled by the spec (the surface holds
   its own window reference); Android selects Vulkan here.
4. **A failed non-probe recreate leaves the presentation released until the next lifecycle event.**
   Deliberate: the plan declines a retry, and the docs now say what statelessness covers (a missed
   signal) and what it does not (a failed act).
5. **The `systems-perf-lead` co-sign** the studio names as SAFETY-GATE's formal clearance was not
   sought, because the diff adds no `unsafe` (the two `borrow_raw` lines are unchanged context under
   a rewritten comment). Stated so the gate's denominator is honest.
6. Pre-existing, out of scope: the Android `callbacks().clear()` site question, the three
   unbuildable Android example manifests, `flui-app` in `cross-typecheck`, the 7 Android-target
   warnings, the stale implementor count in `docs/runtime-contract.toml`.

## Next

`/review` if a deeper audit of the renderer is wanted before merge; otherwise commit on this
branch with `Refs #1146` (the issue is closed by the merge, not by prose), and open the follow-up for
residual 1 before the change ships to a device.
