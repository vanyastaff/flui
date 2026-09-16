# #1146 — drop the wgpu surface on Paused/TerminateWindow, re-acquire on Resume

Review mode: **full** (new public trait method on `PlatformWindow`, cross-crate ripple through
`flui-engine` + `flui-platform` + `flui-app`, and an ADR amendment). No `unsafe` added, no new
dependency. Phase 2.5 review status: all three lenses returned `RESHAPE NEEDED`, with `api-design-lead`
reporting four blocking findings, `harsh-critic` six, and `ollama-lens` five (folded last and marked
**[lens]**); every finding is verified against the tree or the pinned dependency source before folding.
`[lens]` finding 1 is the one that changed the design a second time: `on_ready` fires at the first
`Resume`, which is before Android can have attached a window, so under the ordering the glue indicates
the bootstrap probe fails and the Android runner panics at startup on today's `main`. The reshape is in
A2b, and it is written to be correct under either order. A fourth, narrower pass then ran over the
reshaped plan and returned `RESHAPE NEEDED` on four claim-level findings with the design's core
verified intact (see "The delta pass"); folding them moved one arm of the edit map and three claims,
and left the protocol alone. Testing the plan's own "that check is unavailable" claim afterwards
found the Android runner does not compile on `main` at all, which makes this diff carry a compile fix
as well (correction item 7, A9).

## One correction to the issue's own framing

The issue says `MainEvent::TerminateWindow`/`InitWindow` are "unhandled (~L310-354)" and that a
`SurfaceLost` after resume never sets the device-lost flag. Both are true, and the first is
verifiable in this tree at one place: `platforms/android/mod.rs`'s `poll_events` match handles
`Resume`, `Pause`, `Destroy`, `WindowResized`, `GainedFocus`, `LostFocus`, `ConfigChanged` and
`LowMemory`, and ends with a catch-all `_ => {}` that swallows both window events silently. What
the issue does not say, and what is the reason the defect survived #1145, is that **seven shipped
passages assert a mechanism that `android-activity` 0.6.1 does not implement**:

1. `docs/adr/ADR-0063-…md`, the defect narrative under "Re-reading the code before designing the
   fix": it records the Android half as "`window_handle()` answers `HandleError::Unavailable` between
   `Paused` and `Resumed`, and a *different* `ANativeWindow` after resume", and credits
   device-loss recovery with having "rebuilt the surface against the pointer captured at
   construction". The first half is the false mechanism; the attribution in the second is loose,
   since `AndroidWindow::window_handle()` re-queries the pointer on every call and the capture lives
   inside `wgpu::Surface`.
2. The same ADR's last bullet of Decision 5: "Android already conforms
   (`native_window()` is `None` while paused)."
3. Its Consequences → Negative list, the "Still open" bullet: "Android does not drop its surface on
   `Paused`/`TerminateWindow` (the market shape, filed as a follow-up with the survey's citations)".
   **[lens] That bullet is not wrong about the placement, which is this change's own placement; my
   earlier note misread it as calling the mapping unsafe by accident.** What it needs from this diff
   is closure rather than a mechanism fix: the amendment records that this change drops on both
   events, states why the pair is required rather than optional (`TerminateWindow` is where the
   handle actually dies, and `Pause` alone is insufficient), and retires the follow-up reference.
4. `crates/flui-platform/src/traits/window.rs`, `window_handle`'s MUST doc: "(Android between
   `Paused` and `Resumed`)", and the Android bullet under "How each backend satisfies this".
5. `crates/flui-platform/src/platforms/android/window.rs`, the `window_handle` comment, whose
   justification is a citation of item 6 below, plus the `unsafe` SAFETY comment two lines later.
6. `crates/flui-platform/src/platforms/android/mod.rs`, the module doc's `# Surface Lifecycle`
   section and the `MainEvent::Paused -> surface becomes invalid` line in its Architecture diagram.
7. The same file's two runtime `tracing::info!` strings in `poll_events`, which assert "native
   window available" on `Resume` and "native window may become invalid" on `Pause`. These are the
   only assertions in this family that a human actually reads at the moment they are checking the
   behavior, and they say the opposite of what the new `debug!` line beside them will report, so
   leaving them would put the on-device verification story (below) in conflict with its own log.

Items 5 and 6 cite each other, so the pair is two unverified assertions agreeing.

**The dependency says so itself, which is stronger evidence than anything read from its internals.**
`android-activity` 0.6.1's `MainEvent` declares both variants with a doc comment that states this
exact contract (`src/lib.rs`): `InitWindow` is "a new `NativeWindow` is ready for use. Upon
receiving this command, `AndroidApp::native_window()` will return the new window"; `TerminateWindow`
is "the existing `NativeWindow` needs to be terminated. Upon receiving this command,
`AndroidApp::native_window()` **still returns the existing window**; after returning from the
`AndroidApp::poll_events()` callback then `AndroidApp::native_window()` will return `None`." That
sentence is the reason the drop belongs in the `TerminateWindow` callback and not on `Pause`, and it
is a documented public contract rather than an inference from control flow. Both variants are
`#[non_exhaustive]`, so the match arms must be written `MainEvent::TerminateWindow { .. }` and
`MainEvent::InitWindow { .. }`.

Read against `android-activity` 0.6.1's `native_activity/glue.rs`: the native window is cleared only
by `AppCmd::TermWindow`, applied in `post_exec_cmd`, i.e. **after** the `TerminateWindow` callback
returns. `AppCmd::InitWindow` is applied in `pre_exec_cmd`, so the new window is already set
**before** the `InitWindow` callback runs. `MainEvent::Pause` does not touch the window at all. So
`native_window()` is non-`None` for the whole pause unless the activity was actually recreated, and
"Android already conforms" is false in exactly the case that matters.

Five consequences, all load-bearing, plus one the issue's own scope misses:

1. **Keying the drop on `Pause` alone would be unsafe by accident.** The surface must be dropped
   inside the `TerminateWindow` callback, because that is the last moment the handle behind it is
   still valid.
2. **Correcting those seven passages is part of the fix, not a tidy-up.** They are what made the
   Android half of #1043 look finished, and leaving them re-arms the same trap.
3. **[review] The `unsafe` SAFETY comment at `android/window.rs` is false in its second sentence**
   ("AndroidWindow is only used within that lifecycle window"), since the runner holds and queries
   `AndroidWindow` while paused. The stated range is *narrower* than the true valid range, so the
   block is not unsound; the comment is wrong. The correction routes through `unsafe-auditor`'s
   read in Phase 5b, and this change adds no `unsafe`.
4. **[review] Today nothing on Android drops the surface at a defined moment either, and the reason
   is a registration cycle.** `bootstrap_android` registers `on_request_frame` on the window
   (`runner/android.rs`), and in `flui-app`'s wiring that closure owns the raster lane and hence the
   renderer, whose `SurfaceLease` retains an `Arc` of the window it was built from. The Windows and
   winit backends describe that same cycle in their comments and break it with
   `callbacks().clear()` in their window-destroy paths; the Android backend calls it nowhere
   (verification note 12). So on Android nothing ends the cycle, the window and the renderer stay
   alive with it, and the surface outlives the `ANativeWindow` it was built from. That is the same
   use-after-free class as #713, and releasing inside `TerminateWindow` is what removes it. This is
   the strongest reason the release belongs there and not on `Pause`: it is the first moment the
   surface's lifetime has a defined end at all.
5. **[review] The false claim is checkable in one line, because `window_handle()` has no gate of its
   own.** `AndroidWindow::window_handle()` is `self.app.native_window().ok_or(HandleError::Unavailable)?`
   and nothing else, so it is `Unavailable` exactly when `native_window()` is `None`, which is the
   dependency fact above. `AndroidWindow::is_visible()` has the identical shape
   (`self.app.native_window().is_some()`), so visibility does not track the pause either. That second
   one matters to this plan directly: it means no visibility-based mechanism can be doing the work of
   suppressing frames during a pause, and the lifecycle gate named in decision I is the only thing
   that does.
6. **[lens] The fix the issue asks for is unreachable on today's `main`, because the runner panics
   during startup.** `on_ready` fires at the first `Resume`, the bootstrap needs a window there, and
   the ordering has not delivered one yet, so `Renderer::new` fails and the process aborts before any
   pause/resume cycle can occur (A2b). A surface-across-pause fix that the app never reaches is not a
   fix, which is why the arm change is in this diff rather than filed as a separate issue; it is one
   line in the same match the change already edits.
7. **The file this change edits to wire the callback does not compile on today's `main`, and that
   was found by compiling it.** `cargo check -p flui-app --target aarch64-linux-android` (with the
   three env vars in verification note 9) reports exactly one error:
   `crates/flui-app/src/app/runner/android.rs:543:9`, where the `on_ready` closure returns
   `bootstrap_android`'s `anyhow::Result<()>` where `PlatformReadyCallback` requires
   `Result<(), BootstrapError>`. The sibling `run_desktop` does it correctly, with the comment
   already explaining the conversion (`bootstrap_desktop(...)?; Ok(())`), so `run_android` lost the
   `?; Ok(())` shape at some point after the last time anyone built for Android. This is the direct
   consequence of verification note 9: the file is compiled by no gate, so nothing ever saw it.
   Folded as acceptance criterion A9 and edit-map row 12, because this change edits that closure's
   file and a diff that leaves it unbuildable is not a diff that can be defended.

## Acceptance criteria

**A1 — the lease can hold a released state, and the drop order survives it.**
`SurfaceLease<S>::surface` becomes `Option<S>`. After `release()`, `surface()` is `None` and the
value is dropped; `target()` still returns the same `Arc`, so a re-acquire needs no new ownership.
A released lease that is dropped still drops its target last.

*Pin:* `flui-engine` unit tests in `surface_lease.rs` (host-run; this file already carries tests for
the target's strong count and the surface-before-target drop order). **[review]** `release()` must
keep `probe`'s two-step protocol doc true, so that doc becomes "build FIRST, commit-or-release
LAST", and `replace_surface`'s pairing with `release()` is stated at both sites.

**A2 — a released windowed renderer skips presentation instead of reporting `SurfaceLost`.**
Given a windowed renderer whose surface is released, an attempt to present returns `Ok(false)` /
`Ok(None)` and does not produce `EngineError::SurfaceLost`. This is not cosmetic:
`RasterOwner::handle_render_failure` classifies `SurfaceLost` as "mint a surface generation, ack
`SurfaceOutdated`, drop the frame", so an ordinary background-to-foreground cycle would churn
generations through a surface that is deliberately gone.

**[review] The released case and the not-windowed case stay distinct, deliberately.** The new
`SurfaceAcquireOutcome::Released` covers "windowed, surface released", which is a legitimate state.
A renderer that owns no window (`OwnedOffscreen`, `SharedServices`) reaching presentation keeps
today's `SurfaceLost`, because that is a program error and staying loud is the point. The
distinction is documented at the enum.

**[review] Two shipped doc contracts gain a third cause.** `Renderer::render_scene`'s doc and the
published `RasterBackend::render_scene` doc both enumerate `Ok(false)` as "no damage, or occluded";
a released surface is a third cause, and both are updated.

*Pin:* `SurfaceAcquireOutcome::Released` yields `Ok(None)` in `acquire_surface_texture_with`, driven
by a scripted `SurfaceAcquireBackend` (existing `surface_acquisition_tests` shape, host-run): it
asserts a released acquire produces no error and never calls `reconfigure`. **[review] The
windowed-versus-offscreen disposition is type-checked only, and no unit test is claimed for it.**
`GpuStackOrigin::OwnedWindowed` holds a real `wgpu::Surface`, so at most two of the three variants
are constructible without a GPU. A truth-table test over the disposition would assert a one-line
`matches!` rather than the wiring that uses it, which is the same reason no predicate helper is added
for it at all (see the note after the edit map). That a released acquire is harmless is the weaker of
the two claims here, because the lifecycle gate (decision I) covers the whole paused interval and the
only span where this disposition can matter is the resume edge: the gate opens at `Resume` and the
surface does not exist again until `InitWindow`, so a frame dispatched in that gap would otherwise be
classified as `SurfaceLost` (decision I).

**A2b — the bootstrap waits for the window, not for the resume. [lens]**
`on_ready` fires at the first `MainEvent::InitWindow { .. }` instead of the first `MainEvent::Resume`,
which is the arm this change adds anyway. That is a bug fix rather than a preference.
`bootstrap_android` builds the renderer inside `on_ready`, and `Renderer::new`'s first act is
`probe_target(&target)?` (`renderer.rs:648` in this tree), before any GPU work. The probe asks
`AndroidWindow::window_handle()`, which is `self.app.native_window().ok_or(HandleError::Unavailable)?`
and nothing else (verification note 14), and the dependency's own doc puts `native_window()` at
`Some` only between `InitWindow` and `TerminateWindow`. At the first `Resume` it is `None`, so the
probe fails, `bootstrap_android` returns that error from its `GPU init failed` arm, `Platform::run`
returns it, and `run_android` reaches `panic!("android bootstrap failed: {err:?}")`
(`runner/android.rs:551`).

**Under the ordering the glue indicates, that panic is the startup path on today's `main`**, so the
Android runner dies before any pause/resume cycle can reach the seam this issue is about. Reading
`glue.rs`, `Resume` reaches the Rust loop first: `on_resume` calls
`set_activity_state(State::Resume)`, which writes the command and parks the Java UI thread until
`pre_exec_cmd` notifies; Android's `handleResumeActivity` calls `performResume` before `wm.addView`,
and the surface appears from `addView` onward through `on_native_window_created` →
`set_window(Some)` → `AppCmd::InitWindow`. Android's own documented ordering says the same thing from
the other side: the surface is not available inside `onResume`.

**[lens] The fix does not rest on that ordering argument, and that is why it is preferred over the
lens's option of moving the initial acquire onto the seam.** Keying `on_ready` on the `InitWindow`
arm is correct whichever order holds, because `pre_exec_cmd(InitWindow)` sets the window *before* its
callback runs (the correction section), so at that callback the probe must succeed. If the reverse
order held, today's code would work and this reshape would still be correct, only unobservable. The
lens's option (a) would also work, but it requires `Renderer::new` to tolerate an unavailable target
for **every** caller, including the desktop backends where no window at bootstrap is a genuine
program error, and it would weaken exactly the loudness A2 relies on. The arm change touches one line
of one match and leaves the renderer's contract alone.

**Option (a) would also have been insufficient on its own, which is the second reason to prefer the
arm change.** The bootstrap's next call after the renderer is
`renderer.resize(phys_size.width.0 as u32, phys_size.height.0 as u32)` (`runner/android.rs:180`), and
`phys_size` comes from `AndroidWindow::physical_size`, which is `native_size()` answering `(0, 0)`
while no window is available (`platforms/android/window.rs`). A renderer reshaped to tolerate an absent
target would therefore have been handed a zero-size resize immediately, and `Surface::configure` panics
on a zero width or height (verification note 16). Under A7's rule the zero would instead be written
into the config and detonate at the first recreate. Waiting for `InitWindow` avoids both, because by
then the window exists and its size is real.

What moves in the tree (edit-map row 6): `should_call_ready = true` leaves the `Resume` arm for the
`InitWindow` arm, and the `if should_call_ready && let Some(ready) = on_ready.take()` block is
unchanged, so it still fires exactly once. The inline comment above that block currently says it
"Fires once, at the first `Resume`" and derives the module doc's `Resumed -> on_ready() -> create
surface` sequence; both are true today and become false with this change, so they are corrected in
the same diff. That is a different category from the seven stale passages, which are false now. The
first `Resume`'s `dispatch_active_status_change(true)` call is untouched (row 12's caution), and
nothing observes the first `InitWindow` through the new callback, because registration happens inside
`on_ready` and `on_ready` runs after `poll_events` returns on that same iteration. It does not need
to: the bootstrap acquire is that acquire.

**[critic] The reshape trades a loud startup failure for a silent one if no window ever arrives, and
the repair is one line in an arm this change already edits.** Under A2b, `on_ready` fires only at the
first `InitWindow`, so if no `InitWindow` ever comes, nothing runs: no window, no realm, no error, no
log past the loop-start line. `MainEvent::Destroy` sets `running` false
(`platforms/android/mod.rs:336-345`), the top-of-loop check exits, and with no `bootstrap_error`
recorded `run` returns `Ok(())` (`:418-421`), which `run_android` treats as success. Today the same
startup state panics (`runner/android.rs:551`), so the reshape would be trading a loud failure for a
silent never-started app. The guard goes in the `MainEvent::Destroy` arm: if `on_ready.is_some()` when
`Destroy` arrives, record a `BootstrapError` instead of exiting clean, which routes the failure
through the `PlatformError::bootstrap` path `run` already returns (`:418-420`) and lands it on the
existing panic. Reachability of the never-`InitWindow` state is UNVERIFIED on this host, so the guard
is justified by failure polarity rather than by a reproduced case: a presentation that never started
must not report success.

**[critic] The reshape's other consumers were checked and are clear.** `install_owner_platform` moves
with the closure at `runner/android.rs:543`, so it is installed whenever `on_ready` runs rather than
only at the first `Resume`; and the lifecycle ladder receives its initial `Resumed` directly from
bootstrap at `:519-532`, so the first `Resume`'s `dispatch_active_status_change(true)` reaching no
registrant is harmless rather than a lost transition.

*Pin:* verifiable only by reading, for the reason in verification note 9. No gate compiles
`runner/android.rs`, and no gate executes `platforms/android`, so nothing on this host can fail if
the arm is set in the wrong place. What the tree can be checked for is that the `InitWindow` arm
exists, that `should_call_ready` is set in exactly one arm, and that the `take()` keeps it once-only.
The ordering argument above is recorded as an inference from two independent sources (the
dependency's synchronization and Android's framework ordering) rather than as a device observation,
and the reshape stands without it, which is the point of choosing it.

**A3 — `Pause`/`TerminateWindow` releases, `Resume`/`InitWindow` re-acquires from the retained
target. [review]**
A new per-window callback carries the signal. On `false` the runner releases the surface. On `true`
it **recreates unconditionally**: any held surface is dropped first, then a fresh one is built by
probing the retained `Arc<dyn WindowTarget>` and using the renderer's own `instance`, `device`,
`queue` and `adapter`. Only the surface is rebuilt, never the stack.

Unconditional recreation is the point of the reshape. A rule of "if a surface is already present,
do nothing" makes correctness depend on having received a `false`, and a `false` can be lost (a
registration that landed in a slot nothing dispatches, an event order the arms do not map, a future
backend arm). The next `true` is then indistinguishable from a redundant one, and the no-op
preserves a surface built from a dead window for the rest of the process's life. Recreating makes
the request stateless: `true` means "a surface valid for the handle available now must exist",
whatever came before.

**[lens][critic] The cost of that is per cycle, not per defect, and the earlier draft's one exemption
was circular.** Because `Pause` maps to `false` unconditionally, every `onPause` pays a release plus a
recreate, including the pauses where the window is never destroyed at all (a dialog over the activity,
multi-window deactivation). That is the steady-state price of not having to know which pause is which.
The per-cycle count is a surface drop, a wasted probe, a surface build and configure, a generation
mint, and a full repaint. The wasted probe is the `Resume` half of the mapping: it fires `true` before
any window exists and reports `Failed(SurfaceTargetUnavailable)` (A8), with the real recreate arriving
at `InitWindow`. The earlier draft priced this as "one surface create on a non-frame path" in the
redundant case only, which is the right actions at the wrong frequency. It also exempted the full
repaint on the grounds that a resumed presentation needs a full frame anyway, and that exemption does
not hold: A4 exists *because* the surface was recreated, so in exactly the dialog and multi-window
cases, where the native window was never destroyed, today's surface still had valid contents and only
incremental damage was needed. On those paths the full repaint is a cost this change adds, and the ADR
decision states it as one.

A recreation that fails leaves the renderer released and reports the failure; it must not panic,
must not set device-lost, and must not propagate beyond the seam. A successful recreation mints a
fresh surface generation through the lane's mailbox counter (`lane.note_surface_recreated()`, the
same act device recovery performs, per ADR-0045 decision 4) in the same lane lock scope, so no
stale in-flight work stays addressed to the destroyed surface.

**[review][lens] The release runs on a path an unbounded wait can see, and that is the accepted
price of dropping at the right moment.** `TerminateWindow` is delivered while the platform UI thread
is parked in `set_window`'s `cond.wait` with no timeout (verification note 10), so the release has to
be fast: no lane wait, no probe, no submit (decision G). What it cannot avoid is the surface's own
drop, and the lens corrected what that costs. Dropping a *configured* `wgpu::Surface` runs
`wgpu-core`'s `Drop for Surface` (`src/instance.rs:720-730`) into `unconfigure`, which reaches
`wgpu-hal`'s `Swapchain::release_resources` (`vulkan/swapchain/native.rs:378-392`) and calls
**`vkDeviceWaitIdle`** before destroying anything, with wgpu-hal's own comment: "there is no way to
portably wait until the presentation work is done, we are forced to wait until the device is idle."
So the unbounded term is a full-device idle wait, not `vkDestroySurfaceKHR`, and the earlier draft's
"no lane wait, no probe, no submit" recorded the cheap half of the cost into ADR-0063 while hiding
the dominant one.

**Which arm pays it is now stated correctly, and the correction removes a comfort the earlier draft
took for granted. [critic]** In the ordinary cycle `Pause` precedes `TerminateWindow`, so the device
wait normally lands on the `Pause` callback and the later `TerminateWindow` release is the idempotent
no-op A3 specifies. But `pre_exec_cmd(Pause)` notifying before its callback does **not** take the wait
off the Android UI thread: `set_activity_state` parks on the same condvar for *every* state
(`glue.rs:511-527`, verified: the wait loop is not conditional on which state it is transitioning
to). So once that notify releases the `Pause` park, the Java thread returns from `onPause` and
immediately reaches `onStop` → `set_activity_state(Stop)`, which writes its command and parks again
until the Rust loop processes `AppCmd::Stop`, which it cannot do until it has finished the `Pause`
callback. The wait therefore sits on the Android UI thread between lifecycle callbacks whichever arm
holds the surface. That is the honest form of the bound, and it strengthens decision G's "must be
fast" rule rather than relaxing it. The `TerminateWindow`-without-`Pause` sequence, where the earlier
draft placed the whole bound, is the rare case: both Android manifests in this repository declare
`configChanges="orientation|keyboardHidden|screenSize"`, so a rotation does not recreate the activity,
and that sequence needs an actual destruction. It is still the case where the native window dies with
no earlier signal, and paying a device wait there is preferable to keeping a surface built from a dead
handle. The alternative, deferring the drop to a later event or to the next frame, is the failure
ADR-0063 already records: flutter#160933 asked for `onSurfaceDestroying` to precede the engine's
`cleanup()`, having found that the after-the-fact callback fires once the native surface is gone. The
recreate is off the wait in the sense that matters, because `pre_exec_cmd(InitWindow)` and
`pre_exec_cmd(Resume)` apply the command and notify before their callbacks run. The bound is stated at
the release site and in the ADR decision, with `vkDeviceWaitIdle` named as the term, so the trade is
visible and correctly attributed rather than incidental.

*Pin:* `pub(super) fn ensure_surface` in `flui-app`'s runner, tested against a scripted
`SurfaceLifecycle` backend (host-run): release is idempotent; a `true` while a surface is present
recreates rather than short-circuiting, and drops the previous surface; a failing recreation is
tolerated as a typed outcome and leaves the state released; a release-then-acquire round trip
records exactly those calls. The `Renderer`-side mechanics need a GPU and are type-checked only.

**A4 — the first frame after a recreate is a full repaint.**
A fresh surface has undefined contents, and the engine's damage tracker is incremental, so resuming
with a small damage region would leave garbage around it. Both layers mark it, mirroring the
existing device-recovery arms: `Renderer::recreate_surface` calls `damage_tracker.mark_full_repaint()`
(as `recover()` does at `crates/flui-engine/src/wgpu/renderer.rs:1111`), and the runner marks the
realm's primary as needing a full repaint (as `Recovered` does in
`render_frame_with_device_recovery`).

*Pin:* the seam returns a typed `SurfaceLifecycleOutcome::Recreated` whose doc states the caller owes
a generation mint and a full repaint, and the scripted-backend test asserts that a successful
recreation yields that variant. The outcome is `#[must_use]`, so a caller cannot silently discard
the obligation and leave a resumed presentation presenting into a surface whose identity nothing has
refreshed.

**A5 — the signal is a per-window callback, and only Android emits it.**
`PlatformWindow::on_surface_status_change(Box<dyn FnMut(bool) + Send>)` with a default no-op body.
**[review] The implementor count is 9, not 11** (`TestWindow`, `MacOSWindow`, `AndroidWindow`,
`WebWindow`, `WinitWindow`, `WindowsWindow`, `MockWindow` in `traits/window.rs`, `MockWindow` in
`platforms/headless`, `StubWindow` in `platforms/winit/control.rs`); two further `rg` hits are
doc-comment text in `shared/handlers.rs`. A defaulted method leaves all 9 unaffected. Dispatch goes
through `WindowCallbacks`' existing FIFO and `CallbackLease`, so a callback registered after
`clear()` is not resurrected.

**[review] The default body's cost is asymmetric and the doc must say so.** A backend that never
emits the signal is harmless: the surface is never released and the window is always treated as
available, which is `on_visibility_status_change`'s own documented shape. A backend that emits
`false` and never `true` is not: the presentation is released, every subsequent frame is a skipped
one, and the loop reports `Ok(false)` forever with a blank window. The concrete in-workspace case is
`crates/flui-app/src/app/window_test_support.rs`'s `TestWindow`, which overrides **no** `on_*`
setter, so a callback registered on it compiles and is silently dropped.

**[review] The parameter stays `bool`, and the growth path is recorded rather than left open.** The
family is 3-for-3 on `bool` (`on_active_status_change`, `on_visibility_status_change`,
`on_hover_status_change`), the signal is genuinely two-state, and a callback's parameter type is a
one-way door: changing it later breaks every registrant's closure. The doc therefore names the
alternative considered (`#[non_exhaustive] enum WindowSurfaceStatus`, the shape these crates already
use at `traits/input.rs`, `traits/owner.rs`, `error.rs`, `raster_options.rs`, `raster_owner.rs`) and
states that a third state arrives as a new callback, following the ADR-0035 split, not as a widened
parameter.

*Pin:* **[review] a host-run wire test through a real `PlatformWindow` implementation, not a direct
`dispatch_*` call.** The headless backend already carries the affordance
(`platforms/headless/platform.rs`'s `simulate_visibility`, `:911`, plus
`impl_window_callback_setters!` at `:1127`), so the plan adds `simulate_surface_status(bool)` beside
it and asserts the registered closure observed the value. That path exercises the trait method, the
macro setter, the slot, and the FIFO drain together, which a direct `dispatch_surface_status_change`
test does not: a setter left out of the macro would make a backend accept and silently drop every
registration, and the direct test cannot see that. The Android arm's mapping (`Resume | InitWindow
=> true`, `Pause | TerminateWindow => false`) is **type-checked by `just cross-typecheck` and
executed by nothing**.

**A6 — the seven stale mechanism passages state what `android-activity` implements.**
Textual criterion; see the correction section. Verified by reading `android-activity` 0.6.1.
**[review] The lease protocol itself is protocol-level, so it gets an amended ADR decision, not only
a text fix.** `docs/PORT.md`'s mapping rule is "protocol-level contracts get an ADR; local ones get
a `## Mapping decisions` entry", and ADR-0063's own Scope line names `SurfaceLease` and decisions
2/4/5. The new decision records the release/recreate protocol and its statelessness; the
`## Mapping decisions` entry in `flui-platform/ARCHITECTURE.md` stays the home for the *callback
shape* and its market lineage.

**A7 — a release must not corrupt the authoritative size, and a recreation must not panic on a
surface whose capabilities differ.**
`MainEvent::WindowResized` can arrive while released. `resize` must still update `self.config`'s
`width`/`height` and the painter, skipping only the `Surface::configure` call
(`renderer.rs`'s `resize` sets the config, configures, resizes the painter, and marks a full
repaint, so only the middle step is conditional); `reconfigure_surface` while released is a
documented no-op rather than `Err(NotInitialized)`, which stays the answer for a renderer that owns
no window.

**[review] `recreate_surface` keeps `width`/`height` and re-derives every capability-dependent
field.** `build_windowed_gpu_stack` derives `usage` (from the surface's `COPY_SRC` support), `format`
(`select_surface_format`), `present_mode` and `alpha_mode` from the freshly created surface's
`get_capabilities(&adapter)`, and takes `width`/`height` from the window. A recreation must do the
same for the first four and must NOT for the last two, because `resize` may have updated them while
released. The failure mode of getting this wrong is loud in one direction and silent in the other:
stale capability fields make `Surface::configure` panic on a surface whose capabilities changed
(which is why `usage` and not only `format` has to be re-derived, and verification note 16 carries
wgpu's own Panics list as the measurement), while re-deriving the size
silently returns to a stale window size. The clean implementation extracts that derivation out of
`build_windowed_gpu_stack` into one shared helper, so the doc there about the single source of
`desired_maximum_frame_latency` and of the config value stays true rather than becoming a second
construction site.

*Pin:* type-checked only; needs a real `Renderer`. Listed because one of its two failure modes is
silent, and it is the only criterion whose failure can be an abort on a device rather than a wrong
frame.

**A8 — a failed lifecycle request is observable to a host test, and the runner owns the response.
[review]**
The seam returns `SurfaceLifecycleOutcome::Failed(EngineError)` rather than swallowing the error
inside the implementation. This follows the sibling seam: `DeviceRecovery::try_recover_device`
returns the error so the caller owns logging. Without the carried source, A3's "reports the failure"
clause has no observable pin, since a scripted backend cannot assert a log line.

**[review] The consumer is a log line, not a counter.** The runner logs `Failed` at `warn` with the
carried error as its `source` field, and does not retry: the next `true` re-asks anyway, because the
verb is stateless (decision J), so a retry loop here would duplicate a property the seam already has.
A counter with no reader is a defect this repository has already paid for, so none is added.

**[review][lens] A `true` that arrives while no window is held is expected, and it happens on
every resume.** `set_activity_state` writes `Resume` independently of any window (verification note
10), so the ordering is legitimate; the seam reports `Failed(SurfaceTargetUnavailable)`, which is
already this crate's documented classification for a suspended target — `Renderer::recover`'s doc
(`crates/flui-engine/src/wgpu/renderer.rs:1047-1051`) names "a destroyed window, a suspended Android
surface" as transient, and `SurfaceLease::probe`'s doc says the same. The runner logs it at
**`trace`, not `debug`**, because the frequency is per-cycle rather than rare: `Resume`'s callback
always precedes `InitWindow`'s (A2b), so the window is always absent at that moment and a `debug`
line there is guaranteed noise on a path that is working as designed. `debug` and `warn` are reserved
for outcomes that are genuinely unexpected, which here means a `Failed` at an `InitWindow`. This is
the one classification the earlier draft got wrong by calling it a log-worthy bug, and the level is
the second thing the lens corrected.

*Pin:* the scripted backend returns a scripted `EngineError` and the test asserts `Failed(source)`
carries it.

**A9 — `crates/flui-app`'s Android runner compiles. [critic]**
`run_android`'s `on_ready` closure returns a `BootstrapError` the way `run_desktop`'s does, so
`cargo check -p flui-app --target aarch64-linux-android` is clean. Today it reports one `E0308` at
`runner/android.rs:543`, which makes this a red→green fix in the ordinary sense rather than a
compile-red formality: the error is the red state, and the command is the same one that shows both.

*Pin:* `env CC_aarch64-linux-android=clang AR_aarch64-linux-android=ar
CFLAGS_aarch64-linux-android="--target=aarch64-linux-android" cargo check -p flui-app --locked
--target aarch64-linux-android` exits 0 (verification note 9 has the measured baseline: the same
command with no edit fails with that one error in 24 s wall, 93 s CPU). The fix matches `run_desktop`'s
existing shape and comment rather than inventing a second conversion.

## What was verified before any code was written

Each item was checked against the tree, the pinned dependency source, or a measurement. Each changes
the plan.

1. **`native_window()` is not cleared by `Pause`** (`android-activity` 0.6.1,
   `native_activity/glue.rs`: `AppCmd::TermWindow` is applied in `post_exec_cmd`, `AppCmd::InitWindow`
   in `pre_exec_cmd`). This is the fact behind correction A6 and behind keying the drop on
   `TerminateWindow`.
2. **Frames are driven *after* `poll_events` returns, never inside it.**
   `AndroidPlatform::run`'s loop body is `poll_events` → `on_ready` (once, at the first
   `MainEvent::InitWindow`, per A2b) → `process_input_events` → `dispatch_request_frame`. The event
   arms only set flags. This makes the new callback's lane access deadlock-free, and it is the reason
   the plan specifies a blocking lock rather than `try_lock` (decision G).
   **[critic] The redraw bit is snapshotted before `poll_events` and taken by every iteration, which
   is what bounds the released interval in decision I.** `should_render` and the bit are read before
   the `poll_events` call (`platforms/android/mod.rs:295-300`, call at `:310`), so an arm's own
   `request_redraw()` cannot dispatch a frame in the iteration that processed it; the take at
   `:401-411` is gated on `resumed` but not on where the bit came from. And the dispatcher reads
   exactly one main command per call: `native_activity/mod.rs:207` is a single `ALooper_pollOnce`,
   and its `LOOPER_ID_MAIN` arm reads one command through `read_cmd()` with no drain loop, so
   `Resume` and `InitWindow` always land in different iterations. Those two facts together are why
   the resume edge is a *possible* dispatch window rather than a guaranteed one, and the earlier
   draft's "at least one iteration per resume runs with the gate open and no surface" is withdrawn
   in decision I.
3. **[lens] The startup order is `Resume` before `InitWindow`, the reverse of what the earlier draft
   asserted, and `on_ready` therefore fires too early to build a renderer.** `glue.rs`'s `on_resume`
   routes through `set_activity_state(Resume)`, which writes the command and parks the Java UI thread
   until `pre_exec_cmd` notifies, while the window only arrives from `set_window(Some)` after
   Android's `performResume` returns and `wm.addView` runs. The dependency's own `native_window()` doc
   agrees: `Some` only between `InitWindow` and `TerminateWindow`. So the `Resume` arm's `on_ready`
   runs with no window, `Renderer::new`'s `probe_target` fails, and the runner panics (A2b). The
   reshape keys `on_ready` on the first `InitWindow` instead, which is correct under either order. The
   first `InitWindow` then triggers the registration rather than preceding it, so the earlier draft's
   "the startup `InitWindow` also precedes registration and costs nothing" is withdrawn: the callback
   does not observe the signal that triggered its own registration, and does not need to, because the
   bootstrap acquire is that acquire.
4. **A successful recreate owes a surface-generation mint, and the engine cannot do it.**
   `note_surface_recreated` (`crates/flui-app/src/app/raster_lane.rs`) mints through the mailbox
   counter on the *lane*, and ADR-0045 decision 4 names surface recreation as a mint site. Recovery
   does this in `render_frame_with_device_recovery`'s `Recovered` arm. The runner must therefore
   call `lane.note_surface_recreated()` after a successful recreate, in the same lane lock scope.
   The plan's first draft missed this, and its absence would have left a resumed presentation's
   surface identity stale.
5. **`flui-app`'s sibling runners deliberately do not register a callback they have no signal for**,
   with a comment saying so (`runner/web.rs` on `on_visibility_status_change`). There is no shared
   registration helper, so registering the surface callback only in the Android runner is the
   established shape, not an omission.
6. **`examples/android_demo` cannot be built by any command.** Its manifest inherits
   `[lints] workspace = true` while the package is commented out of `workspace.members`
   (`Cargo.toml`) and is not in `workspace.exclude`, so cargo refuses before compiling anything:
   "current package believes it's in a workspace when it's not". The `cargo ndk -t arm64-v8a build
   -p flui-android-demo` command the workspace's own NOTE recommends fails the same way. Recorded as
   a named finding with a follow-up rather than fixed here, because each repair (add to members, or
   exclude plus drop the lint inheritance) is its own decision with its own ripple.
7. **[review] `MockWindow` in `platforms/headless` is a real `PlatformWindow` with a `simulate_*`
   affordance and the macro's setters** (`simulate_visibility` at `:911`,
   `impl_window_callback_setters!` at `:1127`, exercised at `:1907`), and `flui-platform`'s host
   suite runs it under the CI `test` job's `FLUI_HEADLESS=1` step. This is what upgrades A5's pin
   from a direct dispatch call to an executable wire test.
8. **[review] `OwnedWindowed` cannot be constructed in a host test.** It holds
   `SurfaceLease<wgpu::Surface<'static>>`, built only in `Renderer::new` via
   `build_windowed_gpu_stack`, and the GPU-free double in `wgpu/fake_window_target.rs` hands out a
   bare Xlib id. Any "all three variants" test claim is unachievable and A2 states 2-of-3 instead.
9. **[review] `cross-typecheck` compiles `flui-platform` and nothing else.** All three lines of
    `justfile`'s `cross-typecheck` and all three steps of CI's job are
    `cargo clippy -p flui-platform … --target <triple>`. `flui-app` is never cross-compiled, and
    `runner/mod.rs` gates `mod android` on `target_os = "android"`, so **the Android runner is
    compiled by no gate**: not by `just ci` and not by `cross-typecheck`. This is why the seam and
    its tests must live outside that module (row 10) and why the plan no longer describes row 12 as
    "type-checked".
    **[critic] The repair is available after all, and the measurement says so: the NDK is not
    required to *type-check* this target, only to link it.** `cargo check -p flui-app --locked
    --target aarch64-linux-android` fails in `psm`'s build script (`stacker` ← `flui-rendering` ←
    `flui-app`) only because `cc-rs` looks for `aarch64-linux-android-clang` and
    `aarch64-linux-android-ar`. Point it at the host tools with a target flag and it assembles fine:
    `CC_aarch64-linux-android=clang AR_aarch64-linux-android=ar
    CFLAGS_aarch64-linux-android="--target=aarch64-linux-android"`. With those three set, the whole
    `flui-app` tree checks for Android on this host in 24 s wall (93 s CPU over 8 jobs, sccache warm;
    the run compiled 48 crates), and it immediately reports a real pre-existing error in the file this
    change edits (correction item 7, A9). So the earlier draft's "the obvious repair is unavailable"
    is withdrawn: what is undecided is whether
    `just cross-typecheck` and CI should carry a cross-`cc` configuration for a lint job, which is a
    toolchain decision, not an impossibility. The NDK is still what a real Android *build* needs, so
    the follow-up item is now about linking and about CI's toolchain, not about checking. This tier's
    strength statement is upgraded accordingly: row 12 is compiled by a command any host can run,
    and executed by nothing.
10. **[review] The release path is the one that runs on an unbounded wait, and the recreate is not.**
    In `android-activity` 0.6.1's `glue.rs`: `set_window` (`:482-504`) writes `TermWindow` then
    `InitWindow` and parks on `cond.wait` until `window == pending_window`;
    `pre_exec_cmd(TermWindow)` has no arm at all, so `post_exec_cmd(TermWindow)` (`:645-652`) is what
    sets `window = None` **and** notifies. The `TerminateWindow` callback therefore runs while the
    Java UI thread is parked with no timeout (its only escape is
    `notify_main_thread_stopped_running`). `pre_exec_cmd(InitWindow)` (`:610-616`) and
    `pre_exec_cmd(Resume)` (`:616-629`) notify **before** the callback runs, so the recreate path is
    off that wait. The same source shows `set_activity_state` (`:511-527`) writes `Resume`
    independently of any window, which makes `true`-with-no-window a legitimate ordering rather than
    a bug.
    **[review] The dispatcher confirms the same order, and the two variants are unit-structs.** The
    loop that yields these events is `native_activity/mod.rs`'s `LOOPER_ID_MAIN` arm: it reads the
    command, maps `AppCmd::TermWindow`/`InitWindow` to `MainEvent::TerminateWindow {}`/`InitWindow {}`
    (`:247-248`), calls `pre_exec_cmd` (`:277`), invokes the callback (`:285`), then calls
    `post_exec_cmd` (`:289`), in that order for every command. So "the `TerminateWindow` callback is
    the last moment the handle is valid" is a property of the dispatch loop, not of one absent arm
    being lucky, and the `InitWindow` callback likewise runs after the new window is already set.
    Both variants are declared as unit-structs in `lib.rs` (`:470`, `:477`) and both are
    `#[non_exhaustive]`, so the match arms take `{ .. }`.
11. **[review] The live-smoke harnesses' `surface_released` assertion keeps its meaning on desktop.**
   Both harnesses run the desktop demo, no desktop backend emits the new signal, and only
   `bootstrap_android` calls the release path, so the lease-drop line still fires exactly once at
   window close there. That is why the plan adds distinctly named events for the explicit release
   (`surface_released_by_owner`) and the recreate (`surface_recreated`) instead of reusing the drop
   line's name, and why the ADR states the boundary rather than re-pointing the harness.
12. **[review] The callback-clear census, and Android's gap in it.** `callbacks().clear()` appears at
    five sites across `flui-platform`'s backends (`platforms/macos/window.rs:1548`,
    `platforms/windows/platform.rs:751`, `platforms/winit/window.rs:138`,
    `platforms/winit/platform.rs:1929` and `:2015`, and `platforms/headless/platform.rs:780`) and in
    none of `platforms/android/`. Four of those comments give the same reason: without the clear, the
    registration cycle window → callbacks → frame callback → raster lane → renderer → surface →
    `Arc<Window>` is never broken, and the Windows and winit comments add that the surface must be
    destroyed while the native window behind the renderer's retained target is still valid, citing
    ADR-0063 and the post-quit SIGSEGV of #713. That is also why `bootstrap_android`'s
    `on_request_frame` registration (`runner/android.rs`) pins the renderer on Android today.
    ADR-0063 states the same hazard in its own words: "`WindowCallbacks::clear()` now latches; a
    `CallbackLease` returning after the clear drops its callback instead of restoring it (the #919
    hazard, closed structurally on every backend)". The latch half of that sentence is true and this
    census does not touch it. The "on every backend" half is not: the cycle it describes stays closed
    on Android, which carries no clear site at all. The ADR row's job is to say which half is which.
    Whether Android should gain the site is its own decision, since it changes behavior on a path
    with no executed coverage and interacts with activity recreation, which nothing on this host can
    observe. Filed as a follow-up rather than folded in.
13. **[review] `resize` has exactly one surface-dependent step, and a recreation must re-derive four
    config fields.** `Renderer::resize` sets `config.width`/`height`, calls `Surface::configure`,
    resizes the painter, and marks a full repaint; only the `configure` call touches the surface, so
    A7's requirement there is one guard rather than a rewrite. `build_windowed_gpu_stack` derives
    `usage`, `format`, `present_mode` and `alpha_mode` from the freshly created surface's
    `get_capabilities(&adapter)` and takes `width`/`height` from the window, which is what makes those
    four the fields a recreation must re-derive and the last two the fields it must keep.
14. **[review] `AndroidWindow::window_handle()` has no paused gate, so the shipped claim reduces to
    the dependency fact.** Its body is `self.app.native_window().ok_or(HandleError::Unavailable)?` and
    nothing else (`platforms/android/window.rs`), which is why the comment above it says "no separate
    flag needed". `is_visible()` is `self.app.native_window().is_some()`. Both therefore inherit
    `native_window()`'s behavior exactly, so the seven passages in the correction section are false
    about the method as well as about the reason they give for it.
15. **[review] The pointer is re-queried, not captured, in FLUI's code.** The same comment notes that
    `NativeWindow::window_handle()` "borrows a temporary", i.e. `native_window()` hands back a fresh
    value per call, and `window_handle()` rebuilds its raw handle from that value every time. So
    ADR-0063's "rebuilt the surface against the pointer captured at construction" attributes the
    capture to the wrong layer: FLUI re-queries, and what holds an old handle inside it is
    `wgpu::Surface`, created once from whichever handle it was handed. The amendment corrects that
    attribution rather than preserving it. This is also what makes the re-acquire work without new
    ownership (edit-map row 1): `WindowTarget` is defined as
    `pub trait WindowTarget: HasWindowHandle + HasDisplayHandle + Send + Sync + 'static`
    (`wgpu/window_target.rs`), and both of those methods take `&self`, so the retained
    `Arc<dyn WindowTarget>` is guaranteed by the trait's own shape to answer with whatever handle is
    current at the moment it is asked. A recreate built from that same `Arc` therefore binds to the
    new `ANativeWindow`, and the probe at row 1 is what turns "currently unavailable" into a
    recoverable `Failed` rather than a `CreateSurfaceError`.
16. **[review] `Surface::configure` panics on a stale capability field, in wgpu's own words.** From
    the pinned `wgpu` 30.0.1 source, `src/api/surface.rs`: its `configure` doc carries a "# Panics"
    list with "Texture format requested is unsupported on the surface", "The requested color space
    is unsupported for the requested format", "`config.width` or `config.height` is zero", and "An
    old `SurfaceTexture` is still alive referencing an old surface". Two of those four are the reason
    the four capability fields must be re-derived rather than kept, and the third is a second reason
    `resize` must keep updating the size while released: a zero-size config is a panic, not a
    skipped frame. The same source's `get_current_texture` doc repeats the live-`SurfaceTexture`
    panic, which the release path avoids structurally by taking the lane lock before dropping.

## Edit-site map

| # | File | Change |
|---|------|--------|
| 1 | `crates/flui-engine/src/wgpu/surface_lease.rs` | `surface: Option<S>`, `release()`, `surface() -> Option<&S>`, `has_surface()`; `probe`/`replace_surface` doc pairing; released-lease tests |
| 2 | `crates/flui-engine/src/wgpu/renderer.rs` | `SurfaceAcquireOutcome::Released` (documented against the not-windowed `SurfaceLost`); the acquire disposition, a direct match on `gpu_stack_origin` with no new predicate helper; the `Released` arm in `acquire_surface_texture_with` + scripted test; `release_surface() -> ()`; `recreate_surface() -> EngineResult<()>`; the capability-derivation helper shared with `build_windowed_gpu_stack`; `surface()` doc (the new ambiguity); `resize()`; `reconfigure_surface()`; `render_scene`'s `Ok(false)` doc; two new trace events |
| 3 | `crates/flui-engine/src/raster.rs` | **[review]** the public `RasterBackend::render_scene` doc (`:43-51`) and `reconfigure_surface` doc (`:78-83`), whose contracts A7 changes: a third `Ok(false)` cause, and a released-surface no-op where the doc currently describes only reconfiguration |
| 4 | `crates/flui-platform/src/traits/window.rs` | `on_surface_status_change` default no-op + doc (asymmetric cost, `bool` growth path); correct `window_handle`'s MUST mechanism sentence and its Android bullet |
| 5 | `crates/flui-platform/src/shared/handlers.rs` | field, `new()`, `Debug`, `WindowCallbackEvent::SurfaceStatus(bool)`, `drain_events` arm via `CallbackLease::take`, `dispatch_surface_status_change`, macro setter, `clear` take-all list, the "ten" and "nine slots" doc counts; tests |
| 6 | `crates/flui-platform/src/platforms/android/mod.rs` | `InitWindow`/`TerminateWindow` match arms + dispatch + a `tracing::debug!` naming the event and the signal; **[lens]** `should_call_ready = true` moves out of the `Resume` arm into the `InitWindow` arm (A2b), and the `on_ready` block's "Fires once, at the first `Resume`" comment plus the module doc's `Resumed -> on_ready()` sequence are corrected with it; **[critic]** the `MainEvent::Destroy` arm records a `BootstrapError` when `on_ready` is still untaken, so a window that never arrives fails loudly instead of exiting `Ok(())` (A2b); module doc's `# Surface Lifecycle` section and the diagram line; the two false `info!` strings in the `Resume`/`Pause` arms |
| 7 | `crates/flui-platform/src/platforms/android/window.rs` | the `window_handle` comment and the `unsafe` SAFETY comment's false second sentence |
| 8 | `crates/flui-platform/src/platforms/headless/platform.rs` | `simulate_surface_status(bool)`, wired to the new dispatch; the host-run wire test |
| 9 | `crates/flui-platform/ARCHITECTURE.md` | `## Mapping decisions` entry for the callback shape and its market lineage |
| 10 | `crates/flui-app/src/app/runner/surface_lifecycle.rs` (new) | `pub(super) trait SurfaceLifecycle`, `ensure_surface`, `SurfaceLifecycleOutcome` (incl. `Failed(EngineError)`), scripted-backend tests |
| 11 | `crates/flui-app/src/app/runner/mod.rs` | register the new module **unconditionally**, beside `mod device_recovery;` and `mod lifecycle_ladder;`, with only the `Renderer`-side impl cfg'd |
| 12 | `crates/flui-app/src/app/runner/android.rs` | register the callback once in `bootstrap_android`, wiring the lane (mint) and the realm's full-repaint mark; **[critic]** fix the `on_ready` closure's return type at `:543` to `run_desktop`'s `?; Ok(())` shape, which is an `E0308` on today's `main` and the only thing standing between this file and compiling (A9, correction item 7) |
| 13 | `docs/adr/ADR-0063-the-renderer-owns-its-surface-target.md` | new decision for the release/recreate protocol, its statelessness, the per-cycle cost, and the accepted unbounded-wait bound **named as `vkDeviceWaitIdle`**; correct Decision 5's false bullet; **close** the "Still open" Android bullet (it names the right placement, so it owes closure rather than a mechanism fix) and state why `TerminateWindow` is required and `Pause` alone insufficient; correct the defect narrative's "pointer captured at construction" attribution (verification note 15); reconcile "the #919 hazard, closed structurally on every backend" with the clear census (verification note 12); record the Android callback-clear gap as a stated boundary |
| 14 | `docs/PORT.md` | the `flui-platform` index row's "(partial: Mapping decisions for winit keyboard conversion…)" enumeration |

**[review] Row 11's registration must be unconditional, or A3 and A8's tests are dead in both
directions.** `flui-app`'s runner already carries two seams that are host-compiled on every target
(`device_recovery.rs` and `lifecycle_ladder.rs` are declared with no `cfg`), with the
android-activity dependency confined inside the `Renderer`-side impl and the module's own tests. The
new module copies that: `mod surface_lifecycle;` is unconditional, the trait and the outcome are
portable, and the test file is host-run. `#[cfg(target_os = "android")]` on the module would make the
whole file invisible to every host gate, which is exactly the "a test that cannot run is not
evidence" trap row 12 already documents. It must also survive `wasm-check` and `feature-matrix`,
which compile `flui-app` under other target and feature sets with `-D warnings`.

**[review] The registration is never cleared, and that is now an invariant written at the site.**
The Android backend contains no `callbacks().clear()` call anywhere. Every other windowed backend
clears its slots in its window-destroy path, and the comments there say why: window (through its
callback slots) → frame callback → raster lane → renderer → surface → `Arc<Window>` is a cycle
nothing else breaks, and in two of those comments the surface must be destroyed while the native
window behind the renderer's retained target is still alive (`platforms/windows/platform.rs`,
`platforms/winit/window.rs`). Android has no such site: `MainEvent::Destroy`'s arm (`mod.rs`, which
calls `dispatch_close()`) stops there. Two consequences to state where the registration happens: the
registration is idempotent-by-absence, so a second `bootstrap_android` on the same window would
leave two live leases and the surface would be dropped and rebuilt twice per event; and the missing
clear is *not* added here (see the follow-up list), so that arm stays one line away from becoming a
clear site, which would break the callback for the rest of the window's life, since `clear()`'s own
doc forbids re-registration after a clear.

**[review] Row 12's edit sits immediately beside a dispatch the change must leave untouched.**
`bootstrap_android`'s `on_active_status_change` registration (`runner/android.rs`) carries a comment
explaining that Android's `GainedFocus`/`LostFocus` and `Resume`/`Pause` currently fire the identical
`dispatch_active_status_change`, and that splitting them is "a named follow-up (ADR-0035), not this
PR". The new callback is not that split: it carries surface availability, not app-lifecycle state,
and the ADR-0035 split remains open (see the boundary list). So the two `dispatch_active_status_change`
calls in the `Resume`/`Pause` arms in `mod.rs` stay exactly as they are. Deleting either as
"redundant now" would silently stop the lifecycle ladder from ever reaching `Paused` on Android,
which no gate here would catch, because the ladder's own tests run on the host backends.

Checked and **not** needed structurally: `docs/runtime-contract.toml`'s `PlatformWindow` entry. It
pins existence on `contains = "pub struct WindowId"` in `traits/platform.rs` and enumerates no trait
members, so an added method with a default body cannot stale it; its `thread_affinity` and
`failure_semantics` claims stay true. `just runtime-conformance-check` must stay green. The
`hidden-surface-gating` entry's stale `traits/window.rs:309-329` citation (those lines are
`accessibility`/`get_title`/`set_title`; the winit `Occluded` doc is at `:455-486`) is a citation
tidy in a file this change does not otherwise touch, so it moves to the follow-up list rather than
into this diff.

**[review] `GpuStackOrigin::is_windowed()` is not added.** An earlier draft listed it in row 2, and the
code it would serve shows it has no consumer: every site that has to tell "windowed with a released
surface" from "owns no window" is already a `match` or an `if let` on `gpu_stack_origin` in the same
function, so a predicate helper would exist only to be called once per site. Its only claimed pin was
the truth-table test that row 2 no longer promises, and A2 records why that test is unachievable. A
helper with no consumer and no test is the shape this repository has already paid for.

**[review] Non-windowed dispositions, decided rather than left implicit.** `release_surface()`
returns `()` and is a no-op for a renderer that owns no window, because the requested post-state
("no surface is held") is already true and the lifetime-critical `false` path should be
branch-free. `recreate_surface()` returns `Err(EngineError::NotInitialized)` for that case,
mirroring `reconfigure_surface`, because only a windowed renderer's window lifecycle can
legitimately ask for one, so reaching it there is a program error. Both carry this reasoning at the
site.

**[review] `Renderer::surface()` keeps its signature and gains a doc warning.** Today `None` means
"not windowed"; afterwards it means "not windowed **or** windowed with a released surface". The
distinction is not expressible in the return type without API growth that has no consumer, which is
the same reason ADR-0063 rejected `unsafe fn from_raw_handles`, so the accessor documents the
ambiguity and the runner learns the state through the seam.

## Design decisions

**A. A new callback, not a reuse of `on_active_status_change`.** Android fires
`on_active_status_change` from `GainedFocus`/`LostFocus` as well as `Resume`/`Pause`, so reusing it
would drop and rebuild the surface on a mere focus change. It is also documented in
`bootstrap_android` as an ADR-0035 follow-up, and this change should not spend that record.

**B. A callback, not a polled `is_surface_available()` query.** Every surveyed consumer is
push-shaped: wgpu's own `examples/features/src/framework.rs` drops the surface on suspend and
re-creates it after the resume cycle, egui-wgpu's `Painter::set_window(viewport, None)` and
`Some(window)`, and Flutter's engine-side `SurfaceProducer`. **[lens] Flutter's own history is the
strongest argument for the before-signal form, and the survey corrected the citation**: flutter#160933
("SurfaceProducer needs a 'will destroy' signal, not a 'did destroy'") asked for a callback invoked
*before* the engine's `cleanup()`, naming it `onSurfaceDestroying`, against the after-the-fact
`onSurfaceDestroyed` it was filed about. That is an engine `SurfaceProducer` API, not the framework
`onSurfaceCleanup` the earlier draft named. `TerminateWindow` is exactly that before-signal, which is
why the drop happens inside the callback. **[lens] The dependency's own maintainer states the other
half of this design.** In bevy#6830, `rib` writes that "the only thing that needs to be re-created is
the top-level render target/surface, not all wgpu rendering resources", which is precisely the
surface-only release and recreate specified here, from the person who owns the lifecycle code this
plan reads. Bevy reached that shape the hard way: bevy#9937 first despawned the window on suspend and
spawned a fresh one on resume, then revised to "keep Bevy window, only recreate Winit window and wgpu
surface" (bevy#13689 states the same mechanism). Bevy#9057 names the platform fact underneath it:
Android "destroys all active GL/Vulkan surfaces" when an app is suspended. Both citations belong in
the ADR, and both argue for keeping the presentation and rebuilding only the surface.

**The market's third shape is delegation, and FLUI cannot take it.** Leptos on Android runs as wasm
inside a Tauri WebView, so the WebView owns the surface, `wry` never links wgpu, and the whole
lifecycle reduces to `mWebView.onPause()`/`onResume()` (survey step 4). The framework gets rotation,
multi-window, occlusion and teardown for free and gives up the adapter, the swapchain, and any way to
composite native rendering into its own UI. That trade is open only to a framework whose renderer is
not Rust's, so it is recorded as a boundary rather than as an option, and the two shapes that own a
real surface (wgpu's own example and bevy) are the ones this plan follows. [lens] Even the delegating
shape does not escape the problem: a canvas-backed wgpu surface loses its context as
`webglcontextlost` and wgpu's wasm backend does not recover (gfx-rs/wgpu#3679).

**C. Surface-only recreation on resume; `recover()` stays the device-loss path.** wgpu's example
re-creates only the surface and keeps instance/adapter/device. `recover()` rebuilds the entire stack
and is gated on `lane.is_device_lost()`, a flag a suspend never sets, so routing resume through it
would both lose the surface and rebuild a device that was never lost. `recover()` keeps working
unchanged for a genuine device loss while released: it calls `lease.probe()?` first, so it correctly
fails while the target is gone and correctly rebuilds when it is back.

**D. The decision lives in `flui-app`, not in the Android module.** `platforms::android` is
`#[cfg(target_os = "android")]`, so nothing inside it executes on any host. `just cross-typecheck`
does run `--all-targets`, so a `#[cfg(test)] mod` there is *compiled*, but a test that never runs is
not evidence, and shipping one would inflate the pass count. The Android arm is therefore kept to a
four-arm literal match, with its correctness resting on the android-activity reading above plus the
type-check, and stated as such.

**E. A `pub(super)` trait in `flui-app`'s runner, following `DeviceRecovery`.** That is the
established seam for narrowing the concrete `Renderer` without widening the public `RasterBackend`
trait, and it is what makes A3's decision host-testable. `flui-platform` cannot depend on
`flui-engine` (layer edge), so the platform half speaks only `bool`.

**F. The released state lives in the lease, not beside it.** `SurfaceLease` already owns the surface
and the target and already documents the load-bearing field order. A second `Option` in `Renderer`
would split one invariant across two places, and a released lease still has to drop
surface-before-target.

**G. The callback takes the lane with a blocking lock, not `try_lock`.** The frame closure uses
`try_lock` and logs on failure, because its failure mode is "skip this frame, retry next wake",
which is self-healing. The surface callback's failure mode is not: the surface has to be gone before
`TerminateWindow`'s callback returns, and a skipped release is the defect this issue exists to fix.
A blocking lock is deadlock-free on this backend for the checkable reason in verification note 2:
the lane is held only inside `dispatch_request_frame`, which `run`'s loop calls after `poll_events`
returns, on the same thread, and no event arm drives a frame. That reason is written at the call site
so the next person knows what must stay true.

**[lens] The invariant behind it is "nothing off-thread ever holds the lane", and the comment must
say that rather than only the local facts.** The three facts above are all about this backend's loop
shape; any of them can change without the blocking lock becoming visible as a mistake. A second lane
consumer on another thread (a worker service, a future pipelined presentation) would make the
callback's blocking lock a real deadlock, and the release path is the one place where a wrong
assumption is not self-healing. So the call-site comment names the invariant and the class of change
that breaks it, and A3 states it as a requirement of the design rather than as a property of today's
code.

**H. The release is synchronous on the lane; the realm-side bookkeeping is deferred.** The sibling
callbacks all tunnel through `PlatformToUi` (`WindowFocus`, `WindowVisibility`) and it is tempting
to add a `WindowSurfaceStatus` variant for symmetry. It is wrong for the release, because
`dispatch_platform_realm` may queue rather than run inline depending on the dispatcher's phase, and
a queued release happens after the native window is already gone. The lifetime-critical act
therefore goes straight to the lane inside the callback; the deferrable act (the realm's
full-repaint mark) goes through the realm dispatch. The engine's own
`damage_tracker.mark_full_repaint()` is the primary A4 mechanism and runs synchronously before any
frame can be driven, so the realm-side mark's ordering cannot affect completeness.

**I. The lifecycle gate stays the primary mechanism, and the skip path is not merely defense in
depth. [lens][critic]** `Pause` already drives `AppLifecycleState::Paused` through
`emit_lifecycle_transition`, so `frames_enabled()` is false and no frame is driven while suspended.
That gate is neither removed nor weakened, and A2 must not be described as the thing that stops
presentation while paused. The resume edge is a real window, but it is narrower than the earlier
draft's "at least one iteration per resume runs with the gate open and no surface", which is
withdrawn. The verified mechanism (verification note 2): the redraw bit and `should_render` are
snapshotted before `poll_events` is called (`platforms/android/mod.rs:295-300`, call at `:310`), so
the `Resume` arm's own `request_redraw()` cannot dispatch a frame in the iteration that processes
`Resume`, and the dispatcher reads exactly one main command per call, so `Resume` and `InitWindow`
necessarily land in different iterations. In the ordinary case the next iteration takes the bit and
processes `InitWindow` before dispatching, so the released span is frame-free. A frame **can** still
be dispatched while released, bounded by the `Resume` to `InitWindow` gap, whenever an intervening
iteration returns first, and A2's `Released` disposition is what keeps such a frame from being
classified as `SurfaceLost` and churning a surface generation through a surface that is deliberately
gone (that consequence is `raster_owner.rs:1560-1585`, which mints a generation, acks
`SurfaceOutdated` and drops the frame). The gate also does not cover a frame already in flight when
the release lands, which the same disposition covers.

**J. [review] `true` is stateless, `false` is idempotent.** The two verbs are not symmetric on
purpose. `false` asks for a post-state that can already hold, so releasing twice is one release.
`true` asks for a surface valid for the handle available *now*, which is a question about the
present rather than about history, so it recreates whenever it is asked and never consults whether a
surface is already held. This is the single change that removes the "a lost `false` strands a dead
surface forever" failure mode, and it is why the seam verb is `ensure_surface` rather than a status
setter.

**K. [review] `release_surface` and `recreate_surface`, not `reacquire_surface`.** In
`renderer.rs`, "acquire" already means one thing: getting this frame's swapchain texture
(`acquire_surface_texture`, `acquire_surface_texture_with`, `SurfaceAcquireBackend::acquire`). The
object-level verb for a `wgpu::Surface` is create or build, which is also what wgpu, this file's
`build_windowed_gpu_stack`, and `SurfaceLease::replace_surface` already use. Reusing "acquire" for
the surface object would make one word mean two things in the same file.

**L. [review][lens] The callback stays, even though only one backend can emit it and that backend is
compiled by no gate.**
The alternative to weigh is not "let the Android runner drive the seam itself": `flui-app`'s Android
runner has no `poll_events` of its own, because `flui-platform` owns the event loop, so **every** route
requires `flui-platform` to observe the event and the real choice is only what shape the signal takes
on the way out. The earlier draft's premise, that the runner could drive the seam from its own
`poll_events` match and skip the platform change, was wrong on that point, and the survey corrected
it. The shape is a per-window callback rather than a `flui-platform`-internal hook because the signal
belongs to the platform rather than to the app: `TerminateWindow` is a platform lifecycle event, and a
callback is the only shape in which a second backend can deliver it without also changing every
consumer. **[lens] That second backend is no longer hypothetical**: winit#3786 (merged October 2024)
forwards `suspended()`/`resumed()` from Android's `onStop`/`onStart`, so the workspace's own winit
backend is a scheduled emitter for the same `false`/`true` pair, which is a better argument than the
iOS background case the earlier draft rested on. [lens] The second emitter carries its own caution,
and the cautions belong together: Tauri's stack forwards the same signals through `tao`, which
deliberately swallows the first `onResume` and has carried tao#949 since 2024-06-30 reporting that
`Resumed` is never emitted on Android at all, with the maintainer calling that backend "a mess and
barely works". None of that touches winit#3786's merge status; it is the same lesson as this plan's
evidence tiering, that an unexecuted Android event path is where a signal goes missing.

What the manifest does **not** decide is the shape:
`flui-app/Cargo.toml:121` declares `android-activity` alongside `flui-platform/Cargo.toml:106-108`, so
naming its types from the runner is available, and the callback route is a choice about where the
signal belongs rather than a layering necessity. The trade is explicit: this change buys the correct
seam and the before-signal property at the cost of one public method whose only emitter no gate
executes. That is what a `## Mapping decisions` entry is for, and why the entry is written even though
the mapping cannot run here.

## Verification and claim strength

What this host can prove:

- `just ci` green (fmt-check → inventory-check → runtime-conformance-check → port-check → clippy →
  test → test-doc).
- `just cross-typecheck` green, which is the only thing that compiles the Android backend at all
  (and it does compile `#[cfg(test)]` code, since it passes `--all-targets`). The
  `aarch64-linux-android` line was run against the unmodified tree before planning and is green in
  13 s warm, so it is cheap enough to re-run per edit and it is a real baseline rather than an
  assumed one. Its strength is exactly "compiles and lints clean under the workspace lints".
- The host-run unit tests behind A1, A2's `SurfaceAcquireOutcome` half, A3 and A8's seam decision,
  and A5's headless wire test.
- A7's `raster.rs` half is a **public trait doc** (`RasterBackend`), so the doc-only change is
  checkable by reading while the behavior it describes stays type-checked only. A7's rows are listed
  because their failure mode is silent rather than loud.
- **[lens][critic] A2b's arm change sits in tier (a), and row 12 moves from tier (c) to tier (a) with
  it.** `platforms/android/mod.rs` belongs to `flui-platform`, which `just cross-typecheck` compiles
  and lints with `--all-targets` under the Android triple, so where `should_call_ready` is set is at
  least type-checked. What no gate can check is whether that arm is the *right* place, since nothing
  executes it. `runner/android.rs` was in the weakest tier because nothing compiled it; the check in
  verification note 9 now does, in 24 s warm, which is how A9's pre-existing `E0308` was found at all.
  It stays executed by nothing, so the distinction that survives is compiled-versus-executed rather
  than gated-versus-ungated: the PR states the command it used and states that CI does not run it.

What it cannot, and what the PR must say plainly:

- **No executed Android coverage, and the PR must not blur compiled from executed.** (a) The
  `flui-platform` Android arms (rows 6 and 7) and `flui-app`'s Android runner (row 12) are **compiled
  and never executed**: rows 6 and 7 by `just cross-typecheck`, row 12 by the check in verification
  note 9 that CI does not run. Their mapping and registration are therefore compiler-checked and
  behaviorally unverified. (b) `Renderer`'s windowed surface-creation mechanics are compiled
  host-side but executed nowhere without a GPU. `examples/android_demo` is in a third tier: compiled
  by no gate, and verification note 6 records that it cannot be built by any command at all, so
  nothing about this change can be checked against it and nothing in it is edited.
- **`GpuStackOrigin::is_windowed()` and the `Renderer`-side surface mechanics have no executed
  pin.** They need a real `wgpu::Surface`, and `flui-engine` has no windowing dev-dependency. This
  is the same gap ADR-0063 recorded for `recover()`, restated rather than narrowed.
- **No Flutter cross-check is available here, and none would apply to this change.** `.flutter` is
  absent from this worktree, so `git -C .flutter describe --tags` cannot run and no citation could be
  checked against the pinned tag even if the clone were present. More to the point, it would not
  settle this change: the sparse checkout AGENTS.md prescribes is `packages/flutter/lib` plus
  `packages/flutter/test`, the *framework*, while the Android surface contract lives on the engine
  side (`FlutterSurfaceView`, `SurfaceProducer`) in a different repository. The framework reference is
  silent on this contract by scope rather than by omission, which is what the market survey is for,
  and why the ADR plus the `## Mapping decisions` entry carry the accounting this change owes.
- The red-first evidence is **compile-red** for the additive API pieces (a test naming
  `release_surface` cannot build against `main`) and **mutation-red** for the seam and lease tests:
  after green, invert the production arm each test names, show it fail, restore it. A compile error
  alone is weak evidence and will not be presented as if it were behavioral.
- `MainEvent::Pause` versus `TerminateWindow` on a real device is the one claim that needs a human
  with an emulator. The `tracing::debug!` added in the Android arm names the event and the signal it
  produced, so that verification is one log line rather than an instrumented build.

## Plan review, and what it changed

`api-design-lead` returned `RESHAPE NEEDED`; `harsh-critic` and `ollama-lens` ran the same brief
beside it. The api lens's four blocking findings are folded as follows, each verified in this tree
before folding:

1. `true` cannot mean "skip when a surface is present". Folded into A3 as unconditional recreation,
   with decision J recording the reasoning. This was the one finding that changed the design rather
   than the text.
2. The default no-op's cost is asymmetric and `TestWindow` silently drops registrations. Folded into
   A5's doc requirement, with the `window_test_support.rs` verification.
3. `bool` versus a value enum is a one-way door. Decided in favour of `bool` with the growth path
   recorded in A5's doc.
4. `GpuStackOrigin::is_windowed()`'s claimed GPU-free pin is unachievable. A2 and verification note
   8 now state 2-of-3 and withdraw the claim, and the edit map no longer promises a test for it.

Its fourteen non-blocking findings are folded as the `[review]` markers above. Three were checked
and found partly wrong, and are recorded with the correction rather than absorbed: the "11
implementors" count is 9 (verified by `rg`), the `surface_released` trace line does not degrade on
the harnessed desktop backends (verification note 11), and the released-versus-not-windowed
distinction is kept rather than unified because the not-windowed case is a program error whose
loudness is the point (A2).

### `harsh-critic`'s six findings

Also `RESHAPE NEEDED`. All six were verified against the tree or the pinned dependency source before
being folded. None of them moved the design; they moved what the plan claims about its own evidence,
plus one classification and one missing edit site.

1. **The Android runner is compiled by no gate.** Fixed: verification note 9, and the three-tier
   strength statement under "Verification and claim strength", which the earlier draft had flattened
   into "type-checked and never run".
2. **The seam has to be registered unconditionally.** Fixed: row 11, plus the paragraph on why a
   `#[cfg(target_os = "android")]` module would leave A3 and A8's tests dead in both directions.
3. **The release runs on an unbounded wait, and the recreate does not.** Confirmed in
   `android-activity` 0.6.1's `native_activity/glue.rs`: `set_window` parks on
   `window == pending_window`; `pre_exec_cmd(TermWindow)` has no notify arm, so
   `post_exec_cmd(TermWindow)` clears the window **and** notifies after the callback has returned;
   `pre_exec_cmd(InitWindow)` and `pre_exec_cmd(Resume)` notify before theirs. Fixed: verification
   note 10 and A3's bound paragraph, which names the only unbounded term and justifies accepting it
   from the same before-signal lesson ADR-0063 records.
4. **`crates/flui-engine/src/raster.rs` was missing from the edit map**, and A7 changes a public
   trait method's contract. Fixed: row 3.
5. **A8's outcome had no named consumer, and `true`-with-no-window was misclassified as a bug.**
   Fixed: A8 now names the log line and its level and states why no counter is added, and the
   no-window case is recorded as expected-at-`debug` off the same source as finding 3.
6. **The registration is never cleared on Android.** Fixed: the invariant paragraph after the edit
   map, which also names the `MainEvent::Destroy` arm one line away from becoming a clear site. The
   critic's premise was right and my first fold of it was not: it said only that Android has no clear
   site, and I wrote that winit's two sites were the only ones in the workspace. The census in
   verification note 12 shows five, across four other backends, with the cycle rationale written out
   at each. Correcting that turned the paragraph from an assertion into a cited comparison, and it
   turned up an adjacent verified gap (the missing Android clear site) that is filed as a follow-up
   rather than absorbed.

Two further notes were applied without a design change. The `runtime-contract.toml` stale citation
moves to the follow-up list, since it is a citation tidy in a file this change does not otherwise
touch, while `docs/PORT.md`'s index row stays in the diff because A6 adds the second mapping entry
that row enumerates.

**What the critic could not break**, recorded because it bounds the risk rather than praising the
plan: the unconditional recreate, the surface-only rebuild from the retained target, the generation
mint in the same lane scope, the full repaint on both layers, and `Released` staying distinct from
the not-windowed `SurfaceLost`.

### `ollama-lens`'s findings

The third lens also returned `RESHAPE NEEDED`, and it found a defect in the design's own premises
rather than in its documentation. Five findings, each verified against the source before folding; two
changed substance rather than text.

1. **The startup event order is inverted, and bootstrap panics on device.** Confirmed at three points
   rather than accepted on the lens's word: `glue.rs`'s `set_activity_state(Resume)` parks the Java
   thread and `pre_exec_cmd` is what releases it, so `Resume` is read first; the dependency's own
   `native_window()` doc puts `Some` only between `InitWindow` and `TerminateWindow`; and
   `Renderer::new`'s `probe_target` at `renderer.rs:648` plus the panic at `runner/android.rs:551`
   complete the chain. Fixed: A2b, and verification note 3 rewritten. **The reshape is mine rather
   than the lens's**: it offered "move the initial acquire onto the seam" or "produce device evidence
   for the order", and keying `on_ready` on the first `InitWindow` is narrower than the first option
   and does not need the second, because `pre_exec_cmd(InitWindow)` sets the window before its
   callback runs. It is also the only one of the two that fixes the adjacent zero-size resize: the
   lens's option (a) would have handed `renderer.resize(0, 0)` a renderer that had just been taught to
   tolerate a missing window, and `Surface::configure` panics on a zero dimension (A2b).
2. **The unbounded term is `vkDeviceWaitIdle`, not `vkDestroySurfaceKHR`, and the plan named the
   wrong arm.** Confirmed by reading `wgpu-core`'s `Drop for Surface` into `wgpu-hal`'s
   `release_resources`. Fixed: A3's bound is now two paragraphs, one naming the operation and one
   naming the arm that usually pays it (`Pause`, off the park, with `TerminateWindow`'s release the
   idempotent no-op). This is the finding that would otherwise have written a false mechanism into
   ADR-0063.
3. **The cost accounting was per-defect where the cost is per-cycle, and the no-window `true` belongs
   at `trace`.** Folded into A3's cost paragraph and A8, each with its frequency argument stated
   (every `onPause`, and every resume respectively).
4. **The NDK blocker belongs beside verification note 9**, or the next agent tries to close the
   no-gate status by adding a cross-compile line. Measured on this host and folded.
5. **The re-entry argument needed its invariant, not only its local facts.** Folded into decision G
   and A3.

Its findings 6 and 7 confirmed corrections the plan already carried, with no design change: the stale
mechanism census, the 9-implementor count, `window_test_support.rs`'s absent `on_*` setters, and the
`runtime-contract.toml` "checked and not needed" conclusion. One wording fix came out of them and is
folded as correction item 3: the ADR's "Still open" bullet states the placement correctly, so it owes
closure rather than a mechanism correction.

### The delta pass

A fourth, narrower adversarial pass ran over the plan as reshaped by the three lenses above, on the
grounds that the reshape itself (A2b, the `vkDeviceWaitIdle` bound, the per-cycle cost) had not yet
been attacked. It returned `RESHAPE NEEDED` on four claim-level findings with the design's core
verified intact: release inside `TerminateWindow`, unconditional recreation, and `Released` staying
distinct from `SurfaceLost` were all re-derived and none of them moved. Each finding was verified
against the tree or the pinned source before folding, and two of the pass's own candidate breakages
were withdrawn by the pass with evidence, recorded here rather than dropped: `install_owner_platform`
moves with the closure at `runner/android.rs:543`, and the lifecycle ladder's initial `Resumed` comes
from bootstrap at `:519-532`, so neither depends on `on_ready` firing at the first `Resume`.

1. **A2b's failure polarity.** Folded as the `MainEvent::Destroy` guard in A2b and edit-map row 6.
2. **The park is unconditional, so A3's bound was comforting itself.** The pass read
   `set_activity_state` (`glue.rs:511-527`) as parking only for some states; its wait loop is not
   conditional on the state, so `Pause`'s callback sits on the same parked path as
   `TerminateWindow`'s. Folded: A3's bound paragraph now says the wait is on the Android UI thread
   between lifecycle callbacks whichever arm holds the surface, and that the `TerminateWindow`
   without `Pause` case is rare because both manifests declare
   `configChanges="orientation|keyboardHidden|screenSize"`.
3. **Decision I over-claimed, and the pass's own mechanism for it was impossible.** Its conclusion
   was right: the arm's `request_redraw` does not dispatch in its own iteration. Its parenthetical
   supposed `Resume` and `InitWindow` could be processed in one `poll_events` call, which
   `native_activity/mod.rs:207` rules out (a single `ALooper_pollOnce`, one command, no drain loop).
   Folded with the verified mechanism instead: the pre-`poll_events` redraw-bit snapshot
   (`platforms/android/mod.rs:295-300`) plus the one-command-per-call dispatcher make the released
   span a possible dispatch window rather than a guaranteed one. Decision I, A2's matching sentence
   and verification note 2 are all rewritten to that claim.
4. **A3's cost paragraph exempted the full repaint circularly.** It argued the repaint was not a
   surcharge because a resumed presentation needs a full frame anyway, which assumes the very
   recreate A4 exists to justify. Folded: the exemption is replaced with the case that breaks it (a
   dialog or a multi-window deactivation, where the native window is never destroyed and today's
   surface still had valid contents), and the per-cycle count now names the wasted `Resume` probe.
   The pass's notification-shade example was dropped rather than folded, because a shade does not
   pause the activity.

**One finding came from testing the plan's own claim instead of trusting it, and it is the largest
one in this section.** Verification note 9 said the NDK made `cargo check -p flui-app --target
aarch64-linux-android` unavailable, so it had never been run. It is available with three env vars, and
running it against the unmodified tree reports an `E0308` in `runner/android.rs`, the file this change
wires the callback into: the Android runner has not compiled since the `on_ready` closure last
matched its siblings (correction item 7, A9). So this change ships a compile fix, note 9 is rewritten,
and tier (b) of the claim-strength section collapses into tier (a) for the right reason rather than
the convenient one.

## Explicitly not in this change

- **Converting `examples/android_demo` to the surface-only path.** It already implements the market
  shape by dropping the whole `Renderer` on `Pause` (`examples/android_demo/src/lib.rs`). Editing it
  here would produce a change no command can compile, let alone run, since the package cannot be
  built at all today (verification note 6).
- **Repairing the three Android example manifests** so they are either members or excluded. Named as
  a finding and filed as a follow-up; it changes what `just ci` compiles, which is a tooling
  decision with its own evidence requirements.
- **A retry loop for a failed recreate.** None is needed: `InitWindow` sets the window before the
  callback runs, so at that signal the probe must succeed. A `true` arriving with no window is the
  legitimate `Resume`-before-`InitWindow` ordering rather than a failure to retry, it is logged at
  `trace` for that reason (A8), and a genuine failure is logged at `warn` with its carried source.
  Neither is a poll.
- **Adding `flui-app` to `just cross-typecheck`.** Verification note 9 measured that it works with
  three env vars pointing `cc-rs` at the host tools, so this is no longer an impossibility, and
  A9's pre-existing error is what the line would have caught. Adding it still changes what a lint job
  requires of a runner's toolchain (and says nothing about linking, which does need the NDK), so it
  is a tooling decision with its own evidence requirements and joins the example-manifest item on the
  follow-up list rather than riding in a change about surface lifetimes.
- **Repairing `hidden-surface-gating`'s stale `traits/window.rs:309-329` citation.** It is a
  citation tidy in `docs/runtime-contract.toml`, a file this change does not otherwise touch, so it
  is filed with the example-manifest follow-up instead of riding in this diff.
- **Adding the missing `callbacks().clear()` to Android's `MainEvent::Destroy` arm.** Verified as a
  gap (verification note 12): every other windowed backend clears its slots in its window-destroy
  path to break the window → callbacks → renderer → surface cycle, and Android does not.
  `bootstrap_android`'s own `on_request_frame` registration is what makes that cycle live there
  today. Filed rather than folded because the right site and timing depend on how activity
  recreation re-registers callbacks, which no gate on this host can observe, and because this change
  already covers the surface-lifetime half of the same hazard by releasing inside
  `TerminateWindow`. The arm is touched in this change only by A2b's bootstrap-error guard, which is a
  different edit with a different reason: it makes a never-started app fail loudly, and it neither
  adds nor depends on a clear.
- **The ADR-0035 lifecycle-callback split** that `bootstrap_android` names as a follow-up. This adds
  a surface-shaped signal, not the app-lifecycle one.
- **Moving `SurfaceLease` to the presentation.** Recorded in #1043's rulings as belonging to #559.
- **Unifying the not-windowed presentation error with the released skip.** Changing
  `OwnedOffscreen`'s existing `SurfaceLost` classification would alter `RasterOwner`'s
  generation-minting behaviour on a path with no coverage of the new behaviour, in a change about
  Android's window lifecycle.
- **Dropping the surface when the app is merely occluded or minimized on desktop.** The desktop
  backends deliver no such signal, and inventing one here would change `hidden-surface-gating`'s
  contract, which already specifies hidden-surface behavior as "zero frames, zero submissions"
  rather than "released surface".
