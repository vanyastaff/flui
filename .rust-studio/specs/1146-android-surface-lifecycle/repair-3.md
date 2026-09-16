# Repair pass 3 — the re-review's advisories

This is the third and last repair dispatch the budget allows. Both re-review lenses returned
`ACCEPTABLE` with no blocking finding; `unsafe-auditor` signed SAFETY-GATE, and the code-quality lens
confirmed B1's rebuild is a complete invalidation (every format-baked GPU object lives in the two
replaced fields). Nothing in passes 1 or 2 is reopened.

What is left is nine advisories. Eight are text, and each is a claim the change makes that a primary
source contradicts or that the change's own sibling text contradicts. One is a small extraction with
no behaviour change. Do all of them, in one pass, and touch no file this brief does not name.

Constraints as before: same worktree, no commits, no stash, no push, `git add` explicit paths only,
nothing under `.rust-studio/specs/` edited, and keep the edit set to exactly the files below.

## The one primary-source fact behind three of the items

The Vulkan specification for `vkCreateAndroidSurfaceKHR` (verified against the Khronos registry page
today) states: "During the lifetime of a surface created using a particular `ANativeWindow` handle
any attempts to create another surface for the same `ANativeWindow` … will fail. In particular, only
one `VkSurfaceKHR` can exist at a time for a given window" (return code
`VK_ERROR_NATIVE_WINDOW_IN_USE_KHR`); and "If successful, `vkCreateAndroidSurfaceKHR` increments the
`ANativeWindow`'s reference count, and `vkDestroySurfaceKHR` will decrement it."

`Renderer::recreate_surface` calls `instance.create_surface` while the previous surface is still
held (its build-first/commit-last order), so on the Vulkan backend:

- a `true` that finds a held surface bound to the **same** `ANativeWindow` is refused at
  `create_surface`, keeps the old (still valid) surface, and reaches the caller's `Failed` arm as a
  `SurfaceCreation` error, where it is logged at the `warn` reserved for a genuine failure;
- build-first/commit-last is sound exactly when the handle changed, which is the lost-`false` path
  the design targets;
- a configured surface holds its own strong reference on the `ANativeWindow` for as long as it
  exists, so on Vulkan the lost-`false` residual is a disconnect on a dead producer, not a dereference
  of freed memory.

State those facts where the current text says otherwise. Do not change the recreate order, and do not
add a "skip if already held" check: the plan settled unconditional recreate, and a same-window refusal
costs one failed create plus a `warn`, which is the price of statelessness and is acceptable. What is
not acceptable is text that costs it as two configures or promises a rebuild the platform refuses.

### T1. ADR-0063 decision 6, the "one ordering pays it twice" bullet

`docs/adr/ADR-0063-the-renderer-owns-its-surface-target.md`. Rewrite the accounting: on the ordering
where a second `true` arrives with the surface already held on the same window, the second recreate
is refused by the platform (one failed `create_surface`, one `warn`), not paid as a second
create/configure pair; two mints and two repaints do not occur because the seam returns `Failed`,
which never mints. Keep the per-edge framing (it is still per edge, and still not per defect). Add
that whether the `InitWindow`-before-`Resume` ordering occurs at all is **unverified**: the
`android-activity` 0.6.1 glue emits `Resume` before `InitWindow` on every path the reviewer traced,
so the sentence may describe an ordering that never happens.

### T2. `Renderer::recreate_surface`'s doc: "always builds a surface"

`crates/flui-engine/src/wgpu/renderer.rs`. Narrow "always builds" to "always attempts": on Android a
second surface on a still-connected window is refused by the platform, so a same-window `true` leaves
the held surface in place and returns `SurfaceCreation`. Add one sentence beside the format-rebuild
guard naming its cost: when the format moved, this is the one mid-life path that constructs nine
pipelines and a glyph atlas synchronously, under the caller's held lane, on the callback thread; it is
the startup cost, paid only when the format moved, and it neither submits nor waits.

### T3. The scripted test's doc

`crates/flui-app/src/app/runner/surface_lifecycle.rs`, the doc of
`an_acquire_request_recreates_over_a_held_surface_and_reports_recreated`. Keep the test; it pins the
seam's contract (the seam never consults what is held). Say in its doc that it pins the *seam*, and
that the real Vulkan backend refuses a second surface on the same window, so the "recreates over a
held surface" outcome is delivered by the platform only when the native handle changed.

### T4. ADR-0063 decision 6, the lost-`false` bullet's `vkDestroySurfaceKHR` marker

Same file as T1. Narrow the "whether a Vulkan driver touches the `ANativeWindow` inside
`vkDestroySurfaceKHR` is unverified" marker: on Vulkan the surface holds its own reference, so the
late drop decrements a count on a window the surface itself kept alive, and the residual is a
disconnect on a dead producer (a logic error, the same class the ADR already names for Win32), not a
use-after-free. Leave the EGL/`gles` path marked unverified, because nobody has read it.

## Text contradicted by a vendored source or by a sibling in the same diff

### T5. `crates/flui-platform/ARCHITECTURE.md`, "they key on `onStop`/`onStart`"

False against `winit` 0.30.13 (`src/platform_impl/android/mod.rs`): it maps `MainEvent::InitWindow`
→ `Resumed` and `MainEvent::TerminateWindow` → `Suspended`; its `Start` and `Stop` arms are
`warn!("TODO: forward …")` stubs. Bevy consumes those same events. ADR-0063 and `traits/window.rs`
in this same diff say the correct thing. Fix the sentence so the three agree.

### T6. `crates/flui-platform/src/traits/window.rs`, `window_handle`'s doc — three precisions

- The MUST paragraph cites only `HasWindowHandle`'s object-lifetime prose. Upstream also makes a
  type-level claim: `WindowHandle<'a>`'s doc says all pointers "are guaranteed to be valid and not
  dangling for the lifetime of the handle", and that is the claim `borrow_raw`'s `# Safety` answers
  to. Add one clause: upstream's handle *type* claims validity for `'a`, the Android backend cannot
  meet that by construction, and the SAFETY block on `AndroidWindow::window_handle` says so.
- The Android bullet's "keeps answering `Ok` for the whole span between a `Pause` and the matching
  `Resume`" is over-broad: a `TerminateWindow` inside that span flips the answer to `Unavailable` from
  `post_exec_cmd(TermWindow)` until `pre_exec_cmd(InitWindow)`. Qualify with the bullet's own earlier
  "ordinary pause" wording (absent a `TerminateWindow`).
- The same bullet's "the driver destroys the swapchain on suspend regardless": a `Pause` that retains
  the window does not destroy the swapchain; it dies with the window at `TerminateWindow`, which the
  sentence's own preceding clauses establish. Say "with the window", not "on suspend".

### T7. `crates/flui-app/src/app/raster_lane.rs`, the emitter census

The module doc's "Generation discipline" list and `note_surface_recreated`'s doc say the availability
signal comes "from `MainEvent::TerminateWindow`/`InitWindow`". `platforms/android/mod.rs` also emits
`false` on `Pause` and `true` on `Resume`, and a `Resume` whose window survived the pause recreates
and mints with no `InitWindow` at all. Name all four, as `traits/window.rs` and the ADR do. This is
the census class B2 itself corrected.

### T8. ADR-0063 decision 5, the clear-site census command

The printed command `rg -n 'callbacks\(\)\.clear\(\)' crates/flui-platform/src/platforms` finds only
the winit files. macOS, Windows and headless call it as a field (`callbacks.clear()`), which that
regex cannot see. The conclusion is true (four backends have a site, Android has none); the command
that claims to prove it does not. Replace it with one that matches both forms, for example
`rg -n 'callbacks(\(\))?\.clear\(\)' crates/flui-platform/src/platforms`, and re-run it to confirm the
file list it prints is the one the sentence claims.

## The one code change: one construction site for the format consumers

### T9. `crates/flui-engine/src/wgpu/renderer.rs`

The painter-plus-offscreen pair "built from one format" now has two construction sites,
`build_windowed_gpu_stack` and the rebuild in `recreate_surface`. That is the drift hazard the same
diff cites to justify `derive_surface_config`. Extract a private helper, shaped like
`fn build_format_consumers(device: &Arc<wgpu::Device>, queue: &Arc<wgpu::Queue>, format: wgpu::TextureFormat, size: (u32, u32)) -> (WgpuPainter, OffscreenRenderer)`
(match the surrounding signatures for the exact types), and call it from both sites, so a third
format consumer added later cannot be missed at one of them. Its doc should say that is why it
exists. **No behaviour change**: the same two constructors, the same arguments, the same order. The
gates are the proof.

## Do not implement

- Any change to the recreate order, or a held-surface short-circuit in `ensure_surface` or
  `recreate_surface`.
- A retry for a refused same-window create.
- Anything under `.rust-studio/specs/`.

## Gates and report

`cargo fmt --all -- --check`, then `just ci`, then the Android check
(`env CC_aarch64-linux-android=clang AR_aarch64-linux-android=ar CFLAGS_aarch64-linux-android="--target=aarch64-linux-android" cargo check -p flui-app --locked --target aarch64-linux-android`),
then `just cross-typecheck`. Report per item what changed and where, quote the gate tails, restate
the final `git diff --stat`, and say explicitly if you did not implement an item. Another session may
hold the package cache; a `Blocking waiting for file lock` line is contention, not a property of the
change.
