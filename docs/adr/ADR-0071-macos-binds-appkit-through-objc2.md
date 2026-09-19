# ADR-0071 — The macOS backend moves to `objc2`, and the Apple backends share one binding stack

- **Status:** Accepted
- **Date:** 2026-09-18
- **Supersedes:** nothing. It completes the migration the macOS module header
  named as future work, and retires the last in-tree user of `objc` 0.2 and
  `cocoa` 0.27.
- **Depends on:** [ADR-0070](ADR-0070-ios-binds-uikit-through-objc2.md) (the iOS
  backend, which chose `objc2` first and shares the stack this record puts macOS
  on).
- **Related:** [ADR-0039](ADR-0039-event-loop-affinity-capability.md) (the
  owner-lane routing every AppKit-messaging body travels through, unchanged),
  [ADR-0063](ADR-0063-the-renderer-owns-its-surface-target.md) (the
  `setReleasedWhenClosed:NO` lifetime rule, preserved).

## Context

`flui-platform`'s macOS backend was built on `cocoa` 0.27 / `objc` 0.2. Its own
module header recorded the problem and the plan:

> *"cocoa deprecates its entire API surface in favor of the objc2 family; this
> backend deliberately stays on the single cocoa/objc stack until a dedicated
> objc2 migration replaces it wholesale."*

`objc` has not released since **2019**, and its README points at `objc2` as its
successor. Every shipping Rust Apple stack has consolidated on `objc2` — winit
(its iOS backend since 0.30), `wgpu-hal`, egui, slint, gpui/Zed. The iOS backend
landed on `objc2` (ADR-0070), which left the crate carrying two Objective-C
binding stacks at once, plus a `build.rs` that existed only to declare the
`cargo-clippy` cfg `objc` 0.2's macros expand.

## Decision

**D1 — the macOS backend binds AppKit through `objc2` 0.6 / `objc2-app-kit`
0.3 / `objc2-foundation` 0.3**, and the `cocoa`/`objc` dependencies are removed
from the crate entirely (they no longer appear in `Cargo.lock`, and no crate in
the workspace depends on them). The `build.rs` that existed only for `objc` 0.2
is deleted.

**D2 — the backend's raw `msg_send!` shape is kept, not replaced with typed
method calls.** `objc2`'s macro accepts a raw `*mut AnyObject` receiver,
`Bool` arguments, and a manual `release`, so the file's already-reviewed safety
shape survives the swap. The reason to prefer the raw form here is concrete:
**`NSWindow` and `NSView` are `MainThreadOnly` in objc2's typed API**, while this
backend constructs test windows on a caller-supplied off-main serial lane
(`MacOSWindow::for_test`, exercised by the owner-lane and routing tests) — a
`MainThreadMarker`-gated method would refuse that. The raw macro also keeps the
owner-lane routing (ADR-0039) and the audited `OwnerLaneId` retain/release tail
byte-identical in structure.

**D3 — `define_class!` is not adopted for the two dynamic classes.** The content
view (`FLUIContentView`) has its eleven `NSTextInputClient` methods registered
from a separate module (`text_input.rs`), which the declarative macro cannot
express across files, and the window delegate (`FLUIWindowDelegate`) is a plain
runtime class. Both use `ClassBuilder`, the direct successor to `objc` 0.2's
`ClassDecl`, keeping the registration code's shape.

## Consequences

**Positive — one binding stack across the crate.** macOS/AppKit and iOS/UIKit
now use the same `objc2` family at the same versions, which is also what
`wgpu-hal` already pins, so no new crate generation enters the tree. The crate's
two-Objective-C-stacks wart is gone.

**Two real defects were found by objc2's runtime verification, both silently
accepted by `objc` 0.2.** objc2's `msg_send!` checks the return type against the
selector's encoding at run time, and it caught:
- `makeFirstResponder:` returns `BOOL`, not `void` — the old code cast it to `()`
  and the `objc` macro did not notice.
- The cursor-icon match's explicit `arrowCursor` arm list became
  `clippy::match_same_arms`-visible, because objc2's `msg_send!` expands
  identically across arms where objc 0.2's did not. The list is intentional
  documentation of which icons resolve to the default arrow and is kept under a
  scoped allow.

This is an argument for the migration beyond maintenance: the newer macro is a
stricter oracle, and it found a latent type error the older one hid.

**Trade-offs, named.**
- **`expect(deprecated)` on the module stays.** It no longer covers `cocoa`'s
  blanket deprecation (that dependency is gone) but a handful of AppKit
  accessors this backend uses are deprecated in the multi-scene era; each such
  use is documented at its call site.
- **The `OwnerLaneId`/`unsafe impl Send`/`Drop` machinery is untouched.** The
  migration is a type swap, not a redesign of the owner-lane lifetime rules; the
  audited safety arguments in `window.rs` still hold verbatim, and the manual
  `release` in the drop tail is still a raw `msg_send!`.
- **`accesskit_macos` 0.27 remains on `objc2` 0.5 / app-kit 0.2**, and winit
  0.30 on 0.5 as well, so a second `objc2` generation is still in the lock even
  though the backend itself is on 0.6. That resolves when those dependencies
  move forward (tracked with the winit 0.31 migration, H10), not by anything
  `flui-platform` can do.

**Replacement coverage — the same probes, now exercising `objc2` end to end.**
The four bundled `.app` probes are unchanged in what they assert and all PASS
after the migration:
- `just macos-frame-pump` — the frame pump survives a display pass.
- `just macos-close-path` — close/teardown ordering and no over-release.
- `just macos-ime` — all eleven `NSTextInputClient` callbacks on a live view.
- `just macos-resize-jitter` — the swapchain texture matches the configured size
  under a resize burst.

Each drives the real backend through the production launch path, so the migrated
`msg_send!` sites (window construction, the delegate methods, the input path)
are executed on a real Mac, not merely type-checked. The five real-`NSPasteboard`
clipboard tests and the `display.rs` bounds arithmetic run in the normal suite.
