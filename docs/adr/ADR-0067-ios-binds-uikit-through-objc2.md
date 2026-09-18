# ADR-0067 — The iOS backend binds UIKit through `objc2`, and the framework's
# loop-exit signal is `applicationWillTerminate:`

- **Status:** Accepted
- **Date:** 2026-09-17
- **Issue:** Cross.P / P5 (`docs/ROADMAP-TRACKER.md`) — the native iOS backend
- **Supersedes:** nothing. It is the first record of the iOS backend, and it
  also fixes the binding-stack direction the macOS backend's own module
  comment already named as future work.
- **Depends on:** [ADR-0039](ADR-0039-event-loop-affinity-capability.md) (the
  owner-thread contract this backend keeps by construction — `UIApplicationMain`
  owns the main thread), [ADR-0045](ADR-0045-raster-lane.md) (the surface
  lifecycle the background/foreground edges drive),
  [ADR-0063](ADR-0063-the-renderer-owns-its-surface-target.md) (the renderer
  owns its surface target, so the window handle path is unchanged).

## Context

`flui-platform`'s iOS backend was a 276-line stub: every `Platform` method
returned `unimplemented!()`, and the module doc's "Future Dependencies" list
named `objc`, `block`, `cocoa-foundation`, and `icrate`. Two facts made those
choices wrong before a line was written.

**1. The binding stack.** `objc` (SSheldon/rust-objc) has not released since
**2019**; its README points at `objc2` as its successor. `cocoa` (servo) is
maintained but is macOS-only — it has **no UIKit surface at all**, so it cannot
express this backend even in principle. `icrate` was a short-lived name for the
objc2 framework crates and is **deprecated** by its own README, which says it
has been split into `objc2-*` and should not be used. Meanwhile every shipping
Rust Apple stack has consolidated on `objc2`: winit (its iOS backend has used
`objc2-ui-kit` since 0.30), wgpu/wgpu-hal, egui, slint, and gpui/Zed. Only
`iced` lags, pinned to `objc2` 0.5 until winit 0.31 ships.

The repository had already recorded this direction without taking it:
`crates/flui-platform/src/platforms/macos/mod.rs` says *"cocoa deprecates its
entire API surface in favor of the objc2 family; this backend deliberately
stays on the single cocoa/objc stack until a dedicated objc2 migration replaces
it wholesale."*

**2. The loop-exit signal.** `UIApplicationMain` **never returns**: it runs the
run loop until the process terminates. Every other backend's runner calls
`teardown_platform_realm()` after `Platform::run(...)` returns (desktop), or
from its own loop's exit path (Android). On iOS there is no "after `run`", so
without a deliberate choice the framework would never receive its loop-exit
signal at all — every hosted realm, service pool, and the platform clipboard
would be leaked for the process's life.

## Decision

**D1 — iOS binds UIKit through `objc2` 0.6 / `objc2-ui-kit` 0.3.** Specifically
`objc2`, `objc2-foundation`, `objc2-ui-kit`, `objc2-quartz-core` (`CADisplayLink`,
`CAMetalLayer`), `objc2-metal`, `block2`, and `dispatch2`. The versions are the
ones already in `Cargo.lock` transitively via `wgpu-hal` 30.0.1, so the backend
introduces no new crate generation. `objc2-ui-kit` exposes **one Cargo feature
per header** and enables all ~456 by default, so this backend sets
`default-features = false` and names the classes it messages — the same choice
`winit-uikit` makes.

*Why not the macOS backend's stack:* `objc` is dead and `cocoa` has no UIKit
bindings, so the choice is not "consistency with macOS" versus "modern" — it is
"a stack that can express the platform" versus one that cannot. The macOS
backend's own module comment commits to migrating the same direction.

*Addendum (2026-09-18):* that migration is now done — macOS moved to `objc2`
too, and the `cocoa`/`objc` crates are gone from the workspace entirely. See
[ADR-0068](ADR-0068-macos-binds-appkit-through-objc2.md); the two Apple backends
now share one binding stack.

**D2 — `applicationWillTerminate:` is the framework's loop-exit signal on
iOS.** The platform fires its registered quit handler from that delegate
method, and the iOS runner runs `teardown_platform_realm()` inside it. This is
the only pre-exit notification iOS sends, and the only place a loop that never
returns can run its exit teardown.

**D3 — the delegate's session state is a thread-local, not a process static.**
`UIApplicationMain` owns the main thread for the process's life, and both the
delegate and the values it reads are reachable only from there. A thread-local
is the owner-affine scope ADR-0027 prefers, it matches the winit backend's own
`ACTIVE_EVENT_LOOP` publication, and it keeps the ambient-reach ratchet
(`docs/runtime-contract.toml`) from gaining a new process-global.

**D4 — the frame source is a `CADisplayLink` on the main run loop, and
`request_redraw` is a *demand signal only*.** The link's callback requests a
frame through the window's callbacks; the background and foreground edges pause
and resume it, so a suspended app does no work. This is the iOS-side counterpart
of Android's `poll_events` loop and macOS's display pass: the platform schedules
the frame, and the framework's transaction runs inside it. `request_redraw`
therefore does **not** dispatch a frame synchronously and does **not** call
`setNeedsDisplay()` — both were tried and both are wrong on this backend:

  - *A synchronous dispatch kills the app.* A static tree runs one frame and
    returns, so it looks fine; an animated tree re-arms from inside that frame
    (its ticker wakes through `request_redraw` again), so the callback drain
    never empties and `didFinishLaunching` never returns to UIKit. iOS's
    scene-create watchdog then terminates the process at ~19.6 s
    (`0x8BADF00D`). Measured on a real simulator; the crash stack ran
    `did_finish_launching → bootstrap_ios → request_redraw → drain_events →
    run_frame`.
  - *`setNeedsDisplay()` paints white over the GPU content.* It asks UIKit to
    repaint the opaque `UIView`'s own (empty) layer, drawing it over the
    `CAMetalLayer` the renderer presents into, so frames arrive and the screen
    stays white. Metal presents a drawable to the layer directly; the view's
    display machinery is not part of this path.

  The caller's `needs_redraw` flag is what the next tick's `wake_action` reads,
  so the request is honoured at the display's own cadence rather than lost.
  Verified after the fix: the animated demo runs at ~60 fps (median 16.68 ms
  frame delta on a 60 Hz panel) and survives indefinitely, and the Material,
  ColoredBox and vertical-slice demos all render.

## Consequences

**Positive.** A real Material application renders on a real iOS simulator
(`just ios-sim`): the widget pipeline, layout, paint, and present all run on
UIKit with no tree-specific plumbing — the demo reuses
`examples/material_demo/tree.rs` unchanged.

**The surface lifecycle is reused, not reinvented.** `on_surface_status_change`
drives the same `ensure_surface` seam Android's `Pause`/`InitWindow` pair does,
so the `CAMetalLayer`-backed wgpu surface is released before the layer behind
it goes away and rebuilt when one returns — the precondition ADR-0045 records
for a suspended backend, reached through the same code.

**Trade-offs, named.**

- **A real device is not covered.** `just ios-sim` runs the simulator slice;
  a device build needs signing, which no CI runner here has. The backend's
  API surface is identical (the targets differ only in the slice), but the
  claim "runs on iOS" is, precisely, "runs on an iOS simulator".
- **CI type-checks iOS but does not execute it.** `cross-typecheck` gained an
  `aarch64-apple-ios` clippy line — the backend had no compile gate at all
  before — so a broken iOS build is loud. No CI job boots a simulator; the
  executing coverage is `just ios-sim`, a macOS-host-only recipe.
- **Two `objc2` generations are in the tree.** `wgpu-hal` already pulls
  `objc2` 0.6.4 / framework 0.3.2, which this backend uses; `winit` 0.30 and
  `accesskit_macos` 0.27 still pull `objc2` 0.5.2 / framework 0.2.2. The
  duplicate resolves when those two move forward (tracked with the winit 0.31
  migration, H10), not by anything this backend can do.
- **`UIScreen.mainScreen` is deprecated in the multi-scene era.** This backend
  presents one full-screen window and never adopts `UIScene`, so the app-wide
  accessor is the honest spelling; adopting scenes (iPadOS multi-window) is a
  separate, larger feature. The module carries an `expect(deprecated)` with
  that reason rather than a scattering of per-call allowances.

**A portability bug found by executing it, and fixed at the engine.**
`Renderer::required_limits` started from `wgpu::Limits::default()`, which is
the desktop baseline and asks for `max_inter_stage_shader_variables: 16`. The
simulator's Metal adapter advertises 15, so device creation failed with
`LimitsExceeded` and every iOS bootstrap died before a device existed. The fix
clamps the requested limits to the adapter's own — a limit above the adapter's
can never succeed, and `max_texture_dimension_2d` was already clamped this way.
This is recorded here because it is the kind of defect that only appears when a
backend is *run*, not type-checked: the whole chain
(`UIApplicationMain` → `didFinishLaunching` → window → Metal adapter) worked,
and device creation was the single blocker.

**Replacement coverage.** `just ios-sim` is the executing proof and is the
replacement for the absent CI execution: it asserts, from the app's own unified
log, that a Metal device was created and a frame rendered, and captures a
screenshot. The `display.rs` bounds arithmetic carries three host-run unit
tests (scale conversion, rounding, non-zero origin). Every class encoding and
selector was checked against `objc2`'s generated bindings rather than from
memory, and the crate compiles clean under `clippy -D warnings` for
`aarch64-apple-ios`.
