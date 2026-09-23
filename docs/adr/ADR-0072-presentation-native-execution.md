# ADR-0072: Native execution is a presentation fact

- **Status:** Accepted
- **Date:** 2026-09-19

## Context

UIKit temporary inactivity is not background suspension. The former implementation
released the surface and paused CADisplayLink on resign-active, then failed to
resume the display link on active unless a foreground notification also arrived.
A native delegate protocol probe demonstrated 23 initial frames followed by zero
new frames after resign-active/active, with an unnecessary surface release.

## Decision

The platform owns `WindowExecutionState`: Running, Suspended and reversible
Detached. It is independent of focus, visibility and GPU readiness. The app keeps
these facts per presentation and derives its existing AppLifecycleState stream;
it folds those local results into the realm scheduler. Terminal close remains a
separate irreversible lifetime fence. No raw platform type is added to the facade:
normal applications observe the existing presentation lifecycle capability.

Host and window restrictions combine: terminal/observed detach wins, then either
suspension, then host and window focus/visibility restrictions. Register callbacks
before reading an owner-thread snapshot of the native facts. The app commits the sampled
facts together before reconciling, avoiding transient Resumed for a suspended
presentation. An unobserved tree has no promised initial Paused notification before
this synchronization; subscribers use the optional snapshot contract.

UIKit resign-active changes focus only. Background commits suspended/hidden facts,
pauses native ticks, then notifies execution, focus, visibility and surface loss.
Foreground requests surface restoration before restoring execution while unfocused.
Running does not mean GPU restoration succeeded: the existing renderer surface and
device-recovery gates remain authoritative. Eligible ticks can drive device recovery;
surface recreation failure currently stays released until another availability
request, with no automatic retry promised by this change.
iOS serializes lifecycle observations in an owner-local queue, independently of the
input/frame callback FIFO. Only background/foreground resource intents advance a
generation. A newer resource intent aborts obsolete continuation work, and a queued
resource intent already superseded is skipped before changing facts or surfaces.
Focus-only notifications retain FIFO order and cannot cancel resource completion;
an active notification queued after foreground therefore observes restored execution.
Background pauses CADisplayLink immediately even when logical delivery is queued,
so a nested UIKit run loop cannot service frames while suspension is pending.

Lifecycle effects use immediate, separately contained callback leases instead of
joining the general input/frame FIFO. This deliberate local ordering override keeps
committed native facts and delivered lifecycle events aligned when the originating
observer panics. The general FIFO's unwind policy is unchanged. Invocation and
retired-capture destruction have separate panic boundaries, and the lifecycle pump
stays active through both. Clear requests fence immediate delivery immediately,
while existing general-FIFO Close/Clear ordering remains intact. Callback storage
is private; callbacks and captures run/drop outside locks. Native observations do
not themselves close lifecycle subscriptions.

This uses Rust's explicit per-window state and the existing UiRealm topology rather
than a process-global mobile pause bit. Apple's UIKit inactive/background distinction
is the behavior being preserved, not an assumed Flutter process lifecycle mapping.

## Verification and limits

The owned UIKit delegate probe verifies real CADisplayLink progress and surface
callback history, plus both foreground/background reentry directions. It is a
protocol test, not an OS transition or GPU presentation test. Focus-only reentry,
opposite resource intents, superseded queued restoration, nested run-loop suspension
and a nested foreground observer panic are covered. A portable callback test verifies
frame-origin delivery before the caller unwinds, without claiming that native frame
callbacks may safely unwind across their Objective-C ABI. Headless callback
and app dispatch tests verify FIFO replacement, close fencing, restrictive host
precedence, public lifecycle snapshots and shared-realm sibling eligibility.

Verified native SDK: Xcode 26.2. This is the foundation for UIScene migration, not
scene support. Scene ownership/disconnect/reattach, safe-area geometry, windowless
owner waking and complete mobile quit/background behavior remain separate work.
OS termination does not guarantee a callback. No plist scene declaration is added
without a scene implementation.

Native iOS `PlatformWindow::close` still inherits the unsupported no-op default,
and the runner has no native close teardown. Terminal fencing is verified through
headless close and app stop, not native UIKit closure. The existing iOS quit flag
is not an admission fence. Those ownership gaps belong to the scene/close work,
and this increment must not be described as complete mobile lifecycle support.


## Sources

- [Apple temporary inactivity](https://developer.apple.com/documentation/uikit/uiapplicationdelegate/applicationwillresignactive(_:))
- [Apple scene migration](https://developer.apple.com/documentation/uikit/transitioning-to-the-uikit-scene-based-life-cycle)
- [Flutter scene migration](https://docs.flutter.dev/release/breaking-changes/uiscenedelegate)

## Amendment (2026-09-20): safe-area geometry has a first implementation

The "Verification and limits" list above is unchanged except for safe-area
geometry, which now has an implementation rather than remaining separate work:
the content-view inset is reported to the presentation it belongs to, and the
root `MediaQuery` became presentation-owned on the way in. That second part is
this ADR's own thesis applied to inherited data — the realm used to hold one
`MediaQuery` scoped to its PRIMARY presentation, so a secondary window's resize
or appearance change landed in the primary presentation's tree. Size, device
pixel ratio, brightness and padding are now written through the addressed
presentation, so a shared realm with several live presentations no longer
misroutes them onto the primary.

The addressing stops at the write side: the root `MediaQueryRoot` is installed
from the `primary()` attach path, the only path that carries content today, so a
non-primary presentation's source is written but not yet read. Consuming each
presentation's own source is part of secondary-window content and remains
separate work; `docs/BETA.md` records the split as a stated limit. Nothing here
is visible to a single-window application, which is every iOS build today, since
the scene policy admits one logical session at a time.

Verified on the iPhone 16e simulator in portrait against the view's own
`safeAreaInsets`; the acceptance record and its stated limits are in
[docs/BETA.md](../BETA.md) § "iOS safe-area layout". Scene
ownership/disconnect/reattach, windowless owner waking, and complete mobile
quit/background behavior remain separate work, and OS termination still does not
guarantee a callback.
