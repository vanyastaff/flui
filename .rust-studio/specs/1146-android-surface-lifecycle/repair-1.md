# Repair pass 1 — findings from stages 5a and 5b

Stage 5a (spec compliance) returned `ACCEPTABLE`. Stage 5b returned `NEEDS WORK` from two of three
lenses. This pass fixes the accepted findings. Spend it as **one** dispatch: fix everything below,
then re-run the gates once.

The same read-only rule as before: write only the files named here, do not commit, do not stash, do
not push, and do not edit anything under `.rust-studio/specs/`.

## Blocking

### B1. The re-derived surface format does not reach its consumers [BUG]

`crates/flui-engine/src/wgpu/renderer.rs::Renderer::recreate_surface` re-derives the whole
`SurfaceConfiguration` through `derive_surface_config` and commits it to `self.config`, justified by
its own doc and by `derive_surface_config`'s doc with "a recreated surface on a new window can report
different capabilities". But `self.painter` and `self.offscreen` are built **once**, at
`build_windowed_gpu_stack`, from the *first* config's format: `WgpuPainter::with_shared_device(...)`
passes `config.format` into `PipelineSet::new`, which bakes it into all nine pipelines, and the
offscreen pool is sized from it too. `Renderer::recover` does the opposite and rebuilds both.

The per-frame format comes from `self.config` and feeds both the render-pass target and the
intermediate pool, so the moment the fresh surface selects a different format, every pipeline's
declared target format disagrees with the attachment the frame renders into. Every frame after the
resume then fails wgpu validation, and this backend's `on_uncaptured_error` handler only logs (it
does not set `device_lost`), so nothing self-heals: a blank window with one `error!` per frame. That
is the defect class this change exists to remove, on the resume edge.

**Fix (a), conditional, mirroring `recover`:** when the freshly derived config's `format` differs
from the format the current painter was built with, rebuild `painter` and `offscreen` from the fresh
format; leave them alone when it is unchanged, so an ordinary resume pays nothing.

If, after reading wgpu's `get_capabilities` semantics and this crate's own call sites, you conclude
the format **provably cannot differ** while the instance and adapter are retained, do not implement
(a). Instead take fix (b): stop re-deriving the format, and correct the doc's capability-difference
claim, which is what makes `Surface::configure`'s panic reachable in the first place. **State which
one you took and the evidence for it**, because the two rest on opposite claims about one fact, and
the current doc asserts the (a) claim.

**Evidence owed:** this is a behaviour change. Reachability needs a GPU plus a surface whose
capability list differs, so a behavioural red test is likely impossible on this host; say so
explicitly rather than faking one. What is required instead: name the structural guard you added
(the conditional that joins the format to its consumers), and name what would settle the behavioural
half (a GPU test configuring a second surface that reports a different format, or a proof that the
format list is invariant for a retained adapter).

### B2. The surface-recreated log line misattributes its cause

`crates/flui-app/src/app/raster_lane.rs::RasterLane::note_surface_recreated` documents itself as
"Records that device-loss recovery rebuilt the surface" and its `tracing::info!` says "surface
recreated by device recovery, generation re-minted". This change adds a **second production caller**
(`crates/flui-app/src/app/runner/android.rs`, the resume path), so on Android the log asserts a cause
that is not the cause. The module doc's "Generation discipline" mint-site list repeats the same
attribution, so the census under-enumerates too.

The plan's "Explicitly not in this change" list does not defer this, and the rest of this change is
a sweep correcting docs it made false. Leaving the one it just made false is inconsistent with its
own premise.

Fix, text only: make the message cause-neutral, and state **both** causes in the doc, naming the same
event pair the rest of the diff uses.

### B3. The `unsafe` site's SAFETY comment asserts a bound the code does not have

`crates/flui-platform/src/platforms/android/window.rs`, the `// SAFETY:` block above
`WindowHandle::borrow_raw` in `AndroidWindow::window_handle`. Its first sentence reads: "the returned
handle borrows from `&self`, so the pointer is valid for as long as the borrow can be held — up to
the next `AppCmd::TermWindow`".

**That is false, and the comment contradicts itself five lines later.** `AndroidPlatform` keeps its
`window: Mutex<Option<Arc<AndroidWindow>>>` across a pause; the `MainEvent::TerminateWindow` arm only
dispatches under that guard and never clears or takes the field; `OwnerPlatform::open_window` is the
sole writer. So nothing ends the `&self` borrow, or the `WindowHandle<'_>` derived from it, when the
command is applied, and `'_` does not encode the pointer's death.

The real bound is the `ANativeWindow` refcount. Verify it in the dependency source before writing:
`android-activity` 0.6.1's glue guard holds the last strong `ndk::NativeWindow` clone, and
`post_exec_cmd(AppCmd::TermWindow)` drops it (`guard.window = None` → `ANativeWindow_release`),
*after* the `TerminateWindow` callback returns. That is why the release must run inside that
callback.

Fix: delete the borrow-lifetime clause. State the bound on the pointer, say explicitly that the
returned `'_` is **not** that bound, keep the operative "nothing may dereference the handle once the
window has been terminated" sentence, and name the consumer obligation it rests on as
consumer-enforced rather than type-enforced.

The same lens withheld the SAFETY gate on this alone, so this item is the one that must be right.

## Accepted advisory findings — fix in this pass

Each is a claim this change made false, or a contract this change introduced. They are one coherent
documentation-and-scope sweep; do them together.

- **A1.** `crates/flui-platform/src/traits/window.rs`, `window_handle`'s MUST doc: the paragraph
  stating that `raw_window_handle::HasWindowHandle`'s own contract ties the returned handle's
  validity to the borrow of `self` is the same false premise as B3, and the Android bullet below it
  corrects only the answer rule. Upstream `HasWindowHandle` says the handle "should last for the
  lifetime of the object", which is an object-lifetime claim, not the pointer's. Reconcile the two
  texts in the same pass as B3.
- **A2.** `crates/flui-platform/src/platforms/android/window.rs`, the comment above the
  `native_window()` query: its "answers `None` exactly when no ANativeWindow is live, which is the
  span between `MainEvent::TerminateWindow` and the next `MainEvent::InitWindow`" is open at both
  ends. During the `TerminateWindow` callback the field is still `Some` (cleared by `post_exec_cmd`
  when the callback returns); during the `InitWindow` callback it is already `Some` (set by
  `pre_exec_cmd` before the callback runs). The SAFETY block below states exactly this, so the
  sentence a reader meets first should carry it too.
- **A3.** `crates/flui-engine/src/error.rs`, `EngineError::SurfaceTargetUnavailable`'s doc: it names
  "an Android activity between `onPause` and the next `onResume`" as a gone-handle case. This change
  corrected the identical claim in two other places; this is the surviving third instance. Name the
  `TerminateWindow` → `InitWindow` span, as its corrected twins now do.
- **A4.** `crates/flui-engine/src/raster.rs`, `RasterBackend::reconfigure_surface`'s doc: it states
  two contradictory normative rules for the same state — a no-op for a backend that "holds no
  surface", then a possible error for "a backend that cannot be windowed at all". The only public
  implementor returns `Err(EngineError::NotInitialized)` for the offscreen origin, so a caller
  cannot write against the contract. Delete one of the two sentences; the released-windowed case is
  the one this change adds and the only one the "deliberate release" clause fits.
- **A5.** `crates/flui-app/src/app/runner/android.rs`, the step-8b surface-status callback: the
  blocking lane guard is held across `dispatch_platform_realm`, which drains the realm queue inline
  and can run a `RealmTask::Frame` on this thread with the lane already held. The frame path takes
  the lane with `try_lock` and self-skips, so the cost is a dropped frame rather than a deadlock, and
  the comment's named invariant ("nothing off-thread ever holds this lane") does not cover this
  same-thread hazard class. The comment's own reason for the wide scope needs the guard only through
  the mint. End the guard scope after the mint and dispatch outside it; the realm half is already
  described as deferrable, so ordering does not change.
- **A6.** `crates/flui-engine/src/wgpu/surface_lease.rs`, test
  `a_released_lease_still_drops_its_target_last`: its second assertion asserts a drop order that is
  unobservable in that state, because `release()` already emptied the surface. Assert the post-drop
  log is exactly `["target"]`, which is what distinguishes a released lease, and hoist the two
  recording fixtures into one module helper shared with the pre-existing
  `drop_order_is_surface_before_target` rather than duplicating ~40 lines.
- **A7.** `docs/adr/ADR-0063-the-renderer-owns-its-surface-target.md`, three corrections:
  (i) decision 6's "`#[must_use]` makes that obligation undiscardable" overclaims, since any `let _ =`
  silences it; state what the marker actually buys. (ii) the new paragraph saying the release's
  unbounded wait "lands on that thread" conflates two threads: `set_window` parks the Java/Android UI
  thread, while the release and its `vkDeviceWaitIdle` run inside the `TerminateWindow` callback on
  the thread executing `poll_events`. Name the consequence the sentence currently omits, that an
  unbounded device-idle wait now sits on the Java UI thread's blocking path, and mark it unverified
  if you cannot measure it. (iii) the claim that a missed `false` cannot strand us covers only the
  stranded-surface state: a missed `false` still leaves the configured `wgpu::Surface` to be dropped
  later, at the next `true`'s `replace_surface`, after the old `ANativeWindow` is gone, and whether a
  Vulkan driver dereferences the handle inside `vkDestroySurfaceKHR` is unverified. Name the residual
  rather than letting statelessness read as covering it.
- **A8.** `crates/flui-engine/src/wgpu/renderer.rs`, `derive_surface_config`'s
  `desired_maximum_frame_latency` note enumerates the sites that re-`configure` and now misses
  `recreate_surface` as a fourth. Add it; that is what the comment's "so the literal never drifts"
  argument is for.
- **A9.** `crates/flui-platform/src/traits/window.rs`, the new method's doc: add one sentence naming
  `impl_window_callback_setters!` as the in-crate mitigation for the silent-drop hazard it already
  describes, so the next backend author has an instruction rather than a warning.

## Declined — do not implement

- A `pub fn has_surface` on `Renderer`. `Renderer::surface`'s `None` now conflates "owns no window"
  with "windowed but released", and no in-tree caller exists; its doc states that. Recorded as
  available if a consumer ever appears.
- Any change to the unexecuted Android arms or the outcome-to-action mapping. No device on this host,
  and the plan declares the boundary.
- Doctests for the new API. Family-wide and pre-existing; every sibling is prose-only.
- The stale "11 implementors" figure in `docs/runtime-contract.toml`. Pre-existing, unrelated to this
  change, and that file is a gate registry — not worth touching for a comment.
- Widening miri's scope. Pre-existing, and CI's miri job is advisory.

## Gates, after the fixes

`just fmt-check`, `just clippy`, focused suites (`cargo nextest run -p flui-engine -p flui-app`, and
`FLUI_HEADLESS=1 cargo nextest run -p flui-platform --all-features`), then `just ci`, then
`just cross-typecheck`, then the Android check:

```
env CC_aarch64-linux-android=clang AR_aarch64-linux-android=ar \
    CFLAGS_aarch64-linux-android="--target=aarch64-linux-android" \
    cargo check -p flui-app --locked --target aarch64-linux-android
```

Other agents and worktrees on this machine run cargo concurrently, so a `Blocking waiting for file
lock` line is the shared package cache; report it as such rather than as a property of this change.

## Report back

For each of B1–B3 and A1–A9: what you changed, in which file, and how you verified it. For B1 say
which of (a) or (b) you took and why, and what remains unverified. Quote the gate tails. Say
explicitly if you did not implement one of them, rather than leaving it silent. Restate the final
`git diff --stat`.
