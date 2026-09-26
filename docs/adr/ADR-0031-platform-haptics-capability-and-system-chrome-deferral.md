# ADR-0031: Platform haptics capability, and system chrome deferral

- **Status:** Accepted
- **Date:** 2026-07-17
- **Amended by:** [ADR-0082](ADR-0082-platform-api-contract-crate.md) (§1–§3: `PlatformHaptics`
  now lives in `flui-platform-api`, re-exported at its old `flui-platform` path; its contract is
  unchanged)

## Context

Flutter's `services` layer groups text input, system chrome and haptics.
FLUI dissolves it into capability traits on `flui-platform` (ADR-0030). This
record delivers haptics and defers system chrome in full rather than doing a
partial pass on both.

## Decision

### 1. `flui_types::HapticFeedback` mirrors Flutter's vocabulary

`Vibrate`, `LightImpact`, `MediumImpact`, `HeavyImpact`, `SelectionClick`,
`SuccessNotification`, `WarningNotification`, `ErrorNotification` — one variant
per Flutter `HapticFeedback` static (`services/haptic_feedback.dart`). The enum
is `#[non_exhaustive]` because upstream has already grown it once (the three
notification kinds came later).

Every variant is fire-and-forget and best-effort: a silent no-op where the
platform, OS version or device cannot do it. That is Flutter's own contract
(its API returns `Future<void>` and never reports "unsupported"), and like
Flutter there is no availability query.

The type lives in `flui-types`, below `flui-platform`, so a widget that fires
feedback (a Material switch or ink response) can name it without depending on
the platform layer — the `ImeEvent` precedent.

### 2. `PlatformHaptics` has one `perform(HapticFeedback)` method

Not eight methods. `PlatformTextInput` has one method per control because its
operations differ in meaning and arguments; every haptic kind is the same
operation with a different kind. With `perform(enum)`, a new kind is a
non-breaking variant instead of a breaking trait method. Different growth
profiles, different shapes, on purpose.

### 3. Reached per window

`PlatformWindow::haptics() -> Option<Arc<dyn PlatformHaptics>>`, default
`None` — the same fallible per-window discovery as `text_input()` and
`display()`. The richest target, Android, is per-`View`
(`View.performHapticFeedback`), so per-window is the closest fit; a backend
with one device-global engine returns the same `Arc` from every window.

### 4. Backends and the app entry point

- **winit:** no override. Desktop winit targets have no haptic hardware; `None`
  is the permanent correct answer, not a stub.
- **Headless:** `FakeHaptics` records every call in order (`calls()`,
  `last()`); `MockWindow` returns the same instance on every `haptics()` call.
- **`flui-app`:** `PresentationState::perform_haptic_feedback` resolves its own
  window's capability through its `Weak` window reference; `UiRealm` forwards
  to it. A closed window or a window without haptics is a silent no-op —
  Flutter's degradation contract, not a gap.

No widget-facing handle exists yet. It arrives with the first widget consumer,
as a lifecycle capability (ADR-0078).

### 5. `PlatformSystemChrome` is deferred in full

None of Flutter's six `SystemChrome` methods has an honest desktop surface:

| Method | Why not now |
|---|---|
| `setPreferredOrientations` | orientation lock is a mobile axis; a silent desktop no-op would mislead, since callers have no reason to expect it to be conditional |
| `setApplicationSwitcherDescription` | already covered by `PlatformWindow::set_title()` |
| `setEnabledSystemUIMode` | mobile system-bar modes; no desktop analogue |
| `restoreSystemUIOverlays` | the undo half of the above |
| `setSystemUIChangeCallback` | notifies platform-initiated system-UI transitions desktop does not have |
| `setSystemUIOverlayStyle` | status/navigation-bar tinting; no desktop system bar |

The deferral reopens when a mobile backend needs system-bar or orientation
control; the UIKit (ADR-0073) and Android backends are where that happens.

**Porting note for that work.** `setSystemUIOverlayStyle` is stateful in
Flutter (`services/system_chrome.dart`). Calls set `_pendingStyle` and schedule
one microtask, so N calls in a microtask collapse into one platform call, and a
call equal to `_latestStyle` is skipped. On `AppLifecycleState.detached`,
`_latestStyle` is cleared so the next call after reattach always resends. The
per-frame reassertion apps rely on comes from the rendering layer
(`AnnotatedRegion<SystemUiOverlayStyle>` re-calls it on repaint), not from
`SystemChrome`. A FLUI port therefore needs a trait method **plus** a
framework-side coalescer with the pending/latest dedup and the
detach-clears-cache rule; a bare `set_overlay_style` mirroring
`PlatformHaptics::perform` would drop both.

## Consequences

- Haptics has a vocabulary, a trait, a recording fake and an app entry point;
  real vibration is unverified until a mobile backend implements the trait.
- Widgets cannot fire haptics until a widget-facing handle lands.
- System chrome has a recorded design constraint waiting for its first mobile
  consumer.

## Alternatives rejected

- **Eight discrete haptic methods.** Every new kind would break the trait.
- **A device-global accessor on `Platform`.** Mismatches Android's per-view
  contract and the per-window discovery template.
- **Stubbing system chrome as desktop no-ops.** Orientation and system-bar
  calls silently doing nothing would misrepresent support.
