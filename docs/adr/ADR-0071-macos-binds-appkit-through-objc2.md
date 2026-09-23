# ADR-0071: Apple backends bind their UI kit through objc2

- **Status:** Accepted
- **Date:** 2026-09-18
- **Absorbs:** ADR-0070

## Context

`flui-platform`'s macOS backend was built on `cocoa` 0.27 / `objc` 0.2, and its
iOS backend was a stub whose module doc planned `objc`, `block`,
`cocoa-foundation` and `icrate`. None of those could carry the work:

- `objc` has not released since 2019; its README points at `objc2`.
- `cocoa` is macOS-only — it has no UIKit surface at all — and deprecates its
  whole API in favor of the `objc2` family.
- `icrate` is deprecated by its own README, split into the `objc2-*` crates.

Every shipping Rust Apple stack has consolidated on `objc2`: winit (its iOS
backend since 0.30), `wgpu-hal`, egui, Slint, GPUI/Zed.

iOS adds a second problem: `UIApplicationMain` never returns. Other runners tear
the framework down after `Platform::run` returns (desktop) or from their own
loop's exit path (Android); iOS has no "after `run`".

## Decision

### 1. One binding stack: `objc2`

Both Apple backends bind through `objc2` 0.6 and the 0.3 framework crates —
`objc2-foundation`, `objc2-app-kit` (macOS), `objc2-ui-kit`,
`objc2-quartz-core`, `objc2-metal` (iOS), plus `block2` and `dispatch2`. These
are the versions `wgpu-hal` already pulls, so no new crate generation enters
the tree. `objc2-ui-kit` enables one feature per header (~456) by default, so
the iOS backend sets `default-features = false` and names the classes it
messages, as winit's UIKit backend does. `cocoa`, `objc` and the `build.rs`
that existed only for `objc` 0.2's macros are gone from the workspace.

### 2. macOS keeps the raw `msg_send!` shape

`objc2`'s macro accepts a raw `*mut AnyObject` receiver, `Bool` arguments and a
manual `release`, so the already-reviewed safety shape survived the swap. The
typed API is not used because **`NSWindow` and `NSView` are `MainThreadOnly`
there**, while this backend builds test windows on a caller-supplied off-main
serial lane (`MacOSWindow::for_test`); a `MainThreadMarker`-gated method would
refuse it. The owner-lane routing (ADR-0039) and the `OwnerLaneId`
retain/release tail stay structurally identical, and the
`setReleasedWhenClosed:NO` lifetime rule (ADR-0063) is preserved.

`FLUIContentView` and `FLUIWindowDelegate` are built with `ClassBuilder`, not
`define_class!`: the content view's eleven `NSTextInputClient` methods
(ADR-0069) are registered from a separate module, which the declarative macro
cannot express across files.

### 3. iOS: `applicationWillTerminate:` is the loop-exit signal

The platform fires its quit handler from that delegate method, and the runner's
framework teardown runs there. It is the only pre-exit notification iOS sends;
OS termination does not guarantee it. Scene-level lifetime and quit fencing are
ADR-0073.

### 4. iOS: delegate session state is a thread-local

`UIApplicationMain` owns the main thread for the process's life, and the
delegate and everything it reads are reachable only there. A thread-local is
the owner-affine scope ADR-0027 prefers and adds no process global.

### 5. iOS: `CADisplayLink` drives frames; `request_redraw` is a demand signal

The link's callback requests a frame through the window's callbacks; the
platform schedules the frame and the framework's transaction runs inside it —
the counterpart of Android's poll loop and macOS's display pass.
`request_redraw` only raises the flag the next tick reads. Two tempting
alternatives are wrong on this backend:

- **Dispatching a frame synchronously.** An animated tree re-arms from inside
  that frame, the callback drain never empties, `didFinishLaunching` never
  returns, and the scene-create watchdog kills the process (`0x8BADF00D`).
- **Calling `setNeedsDisplay()`.** UIKit repaints the opaque view's own empty
  layer over the `CAMetalLayer` the renderer presents into; frames arrive and
  the screen stays white.

Native execution state (inactive vs background) is ADR-0072.

## Consequences

- One Objective-C stack across the crate, at the versions `wgpu-hal` pins.
- `objc2`'s run-time encoding checks are a stricter oracle: they caught
  `makeFirstResponder:` returning `BOOL` where the old code cast it to `()`.
- `winit` 0.30 and `accesskit_macos` 0.27 still pull `objc2` 0.5, so two
  generations remain in the lockfile until those dependencies move.
- The `OwnerLaneId`/`unsafe impl Send`/`Drop` machinery on macOS is unchanged —
  the migration swapped types, not lifetime rules.
- iOS runs on the simulator (`just ios-sim`); device builds need signing no CI
  runner has, and CI type-checks `aarch64-apple-ios` without executing it.
- Running iOS for real exposed that `Renderer::required_limits` started from
  desktop defaults the simulator's Metal adapter rejects; requested limits are
  now clamped to the adapter's.
- The macOS behavior is exercised on a real Mac by the bundled `.app` probes
  (`just macos-frame-pump`, `macos-close-path`, `macos-ime`,
  `macos-resize-jitter`); iOS by `just ios-sim`, which asserts from the unified
  log that a Metal device was created and a frame rendered.

## Alternatives rejected

- **Keep macOS on `cocoa`/`objc` and put only iOS on `objc2`.** `cocoa` cannot
  express UIKit, `objc` is unmaintained, and two binding stacks in one crate
  double the unsafe surface to review.
- **objc2's typed `MainThreadOnly` API on macOS.** Refuses the off-main test
  lane the backend's owner-lane tests rely on.
- **`define_class!` for the macOS dynamic classes.** Cannot span the separate
  text-input module.
