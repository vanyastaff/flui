# Review brief — plan attack for issue #1146

> Historical artifact: this is the brief the three Phase-2.5 lenses and the delta pass were given.
> It predates the reshape, so it names the draft's `reacquire_surface` and
> `SurfaceAcquireOutcome::Suspended`; the plan now says `recreate_surface` and `Released` (decisions
> J and K), and the bootstrap arm moved (A2b). The plan is authoritative where the two differ.

You are attacking a PLAN, before any code is written. Read-only: do not edit any file.

## The artifacts

- Plan under attack: `.rust-studio/specs/1146-android-surface-lifecycle/plan.md`
- Issue: `/dev-task https://github.com/vanyastaff/flui/issues/1146` — title
  "android: drop the wgpu surface on Paused/TerminateWindow and recreate on Resume". Body is on
  GitHub; the plan restates its substance.
- Repository: `/home/vanyastaff/orca/workspaces/flui/android-drop-the-wgpu-surface-on-paused-terminat`
  (a git worktree of `vanyastaff/flui`, branch `vanyastaff/android-drop-the-wgpu-surface-on-paused-terminat`,
  clean, HEAD `4aa8782b`). All commands run from this directory.

## The change in one paragraph

Android's `wgpu::Surface` survives a pause today, because nothing in the Android backend handles
`MainEvent::TerminateWindow`/`InitWindow` and the only path that rebuilds a surface is device-loss
recovery, which a suspend never triggers. The plan makes `SurfaceLease::surface` optional, adds
`Renderer::release_surface`/`reacquire_surface` (surface-only, keeping instance/adapter/device), adds
a `SurfaceAcquireOutcome::Suspended` outcome so a released renderer skips a present instead of
reporting `SurfaceLost`, adds a new `PlatformWindow::on_surface_status_change(bool)` callback that
the Android backend fires on `Pause`/`TerminateWindow` (false) and `Resume`/`InitWindow` (true), wires
it in `flui-app`'s Android runner through a new `pub(super)` `SurfaceLifecycle` seam, and corrects
three shipped documents that assert a mechanism android-activity 0.6.1 does not implement.

## Prior context you should read before judging

- `docs/adr/ADR-0063-the-renderer-owns-its-surface-target.md` — the ADR that owns renderer/surface
  ownership, including its "still open" list naming this exact defect.
- `crates/flui-engine/src/wgpu/surface_lease.rs` — the protocol being changed.
- `crates/flui-app/src/app/runner/device_recovery.rs` — the seam pattern the new seam copies.
- `crates/flui-platform/src/platforms/android/mod.rs` — the edit site.
- `crates/flui-platform/src/shared/handlers.rs` — the callback machinery a new callback must join.
- `AGENTS.md` — the Prime Directive (Flutter is the behavioral reference, the market is surveyed
  before settling, done means verified).

## Options already ruled out (do not re-propose without saying why the ruling is wrong)

1. Reusing `on_active_status_change` for the surface signal — it also fires on focus changes.
2. Widening the public `RasterBackend` trait instead of a `pub(super)` runner seam.
3. Routing resume through the existing `Renderer::recover()` — it rebuilds the whole GPU stack and
   is gated on `is_device_lost()`, which a suspend never sets.
4. A `#[cfg(test)]` unit test inside `platforms::android` — the module is `#[cfg(target_os =
   "android")]`, so its tests compile under `cross-typecheck --all-targets` but never execute
   anywhere. A test that cannot run is not evidence.
5. Converting `examples/android_demo` to the new API — it is outside the workspace and compiled by
   no gate.
6. Polling for surface availability instead of a callback.

## What to attack

Judge the plan on these, hardest first. Be concrete, cite file and symbol, construct failure
scenarios, and give no praise:

- **Is the decomposition right?** Eleven files across three crates and an ADR for this. Is any of it
  unnecessary, or is anything load-bearing missing?
- **The ordering and lifetime hazard.** The surface must be dropped while the native handle is still
  valid. Does the plan drop it at the right moment on each of the four events? What happens if the
  events arrive in an order the plan does not consider?
- **The re-entry hazard.** The new callback runs inside `poll_events` on the owner thread and takes
  the raster lane's lock. Can it deadlock, block, or re-enter a frame? Note that `RasterLane` is
  held behind an `Arc<Mutex<..>>` shared with the frame closure.
- **Idempotence.** The plan makes a re-acquire when a surface is already present a no-op. Under what
  real event sequence does that silently keep a surface built from a dead window?
- **The skip path.** Does `Ok(false)` from `render_scene` while suspended actually reach every
  caller in a state that is safe, or does some caller treat it as an error or as "nothing to do
  forever"?
- **The stale-doc correction.** Is correcting ADR-0063 §5 and two doc comments in scope, and is the
  corrected mechanism claim actually right?
- **The evidence.** The plan admits the Android half is type-checked and never executed. Is the
  proposed evidence the strongest available on this host, or is there a gate/pin the plan missed?
- **Semver and API shape.** A new method with a default body on a public 11-implementor trait.

## Verdict format

Return `ACCEPTABLE`, `RESHAPE NEEDED`, or `BLOCKED`, with a numbered list of concrete reasons. No
praise, no restatement of the plan.
