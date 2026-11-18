# ADR-0369: The macOS host binds AppKit through typed objc2

- **Status:** Implemented (2026-11-17)
- **Date:** 2026-11-17
- **Supersedes:** [ADR-0071](ADR-0071-macos-binds-appkit-through-objc2.md) §2 (macOS keeps
  the raw `msg_send!` shape, and `FLUIContentView` and `FLUIWindowDelegate` are built with
  `ClassBuilder`)
- **Related:** [ADR-0323](ADR-0323-a-value-stays-on-the-owner-thread-unless-its-signature-names-what-crosses.md)
  §1 (state only the owner thread touches), [ADR-0151](ADR-0151-platform-layer-boundary-and-names.md)
  §16 (the OS enters the crate through one boundary), [ADR-0254](ADR-0254-window-contract.md) §5
  (a window is neither `Send` nor `Sync`)

## Context and problem statement

ADR-0071 moved the Apple hosts to `objc2` but kept the macOS window on the raw form: a
`*mut AnyObject` receiver with comma-less `msg_send!`, and the window's delegate and content
view built with `ClassBuilder`, whose ivars were read by name and cast to a boxed Rust value. Its
reason was that `NSWindow` and `NSView` are `MainThreadOnly` in the typed API while test windows
were built on an off-main serial lane. That lane is gone: every AppKit window is made on the main
thread, which a `MainThreadMarker` proves, and a window is neither `Send` nor `Sync`. Meanwhile the
loop delegate, the Metal layer and every iOS class were written with `define_class!`, so the host
carried two binding generations side by side, and each new class copied whichever was nearer.

The raw form also hid two leaks no type caught: the window delegate was made `+1` and never
released (AppKit holds a delegate weakly), and the tracking area's `+1` was never balanced.

## Decision drivers

- One binding generation per host: the next class copies the form that is there.
- Ownership and thread affinity carried by types (`Retained`, `MainThreadOnly`, typed ivars)
  rather than by `SAFETY:` comments over pointer casts.
- Every Objective-C method stays inside the panic boundary that `xtask panic-boundary` checks.

## Considered options

1. Keep the raw form for the window and its two classes (ADR-0071 §2).
2. Classes with `define_class!` and typed ivars; messages typed where objc2-app-kit has them.

## Decision

Option 2. Every class the macOS host registers is a `define_class!` class whose Rust state is
its `ContainedIvars`, and the window holds what it messages as typed, retained objects:

```rust
define_class!(
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FLUIContentView"]
    #[ivars = ContainedIvars<ViewContext>]
    pub(crate) struct FluiContentView;
    // NSResponder overrides, and `unsafe impl NSTextInputClient for FluiContentView`.
);

pub struct MacOSWindow {
    ns_window: ManuallyDrop<Retained<NSWindow>>, // a `FluiWindow`
    view: ManuallyDrop<Retained<FluiContentView>>,
    delegate: OnceCell<Retained<WindowDelegate>>, // AppKit holds it weakly
    main: MainThreadMarker,
    // …
}
```

A raw `*mut AnyObject` remains only where another API takes one: the content view a feature
reaches through `ContentViewRef`, the raw window handle, and `MacOSWindow::ns_window`. A raw
`msg_send!` remains for `super` initializers and getters objc2-app-kit types without the feature
the crate enables (the view's backing layer), and in `window/apple/macos/cursor.rs`, whose
typed getters are non-null where the raw ones answer nil (listed in `docs/gaps.md`).

### Consequences

- The window keeps its delegate from right after it is made until its drop, which detaches it
  and hands it to the autorelease pool: the drop may run inside one of the delegate's methods.
- A window whose construction stops early releases its `NSWindow` instead of leaking it.
- `#![expect(deprecated)]` and the Cocoa-era aliases (`ObjcId`, `NIL`, `YES`, `NO`) are gone
  from the window module.

### Confirmation

The macOS cross type-check (`cargo check --target aarch64-apple-darwin`) compiles the classes and
their ivar types; `xtask panic-boundary` finds every method's body in the boundary; the AppKit
window conformance suite (`appkit_window`) runs the windows on a Mac.

## Pros and cons of the options

### 1. Keeping the raw form

- Good: no migration.
- Bad: two binding generations in one host; ivar casts and manual retain counts the compiler
  cannot check, which leaked the delegate and the tracking area.
- Bad: its reason, an off-main test lane, no longer holds.

### 2. Typed classes and messages

- Good: ownership, thread kind and ivar types are the compiler's; the delegate and tracking area
  leaks are fixed by holding them as `Retained`.
- Good: the same form as the loop delegate, the Metal layer and the iOS host.
- Bad: a typed getter that objc2 declares non-null panics where AppKit answers nil; the cursor
  module stays raw until each such getter is checked.
