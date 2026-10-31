# ADR-0277: A window publishes its insets as one value, and the frame samples the keyboard's motion

- **Status:** Accepted (2026-10-30). Implemented: the value, the capabilities, the frame sampling,
  the recording and the headless script (§1 to §3, §5), and the Android keyboard at rest and its
  system gesture regions (§4). Not implemented: the Android keyboard's motion, and the iOS and
  Web producers (§4).
- **Date:** 2026-10-30
- **Supersedes:** [ADR-0254](ADR-0254-window-contract.md) decision 1 only for the snapshot field
  `safe_area`, which becomes `insets: WindowInsets`.
- **Related:** [ADR-0255](ADR-0255-mobile-and-web-hosts.md) (where each host reads its insets;
  this record is the contract those producers publish into),
  [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md) (one frame transaction: a frame
  lays out with the inset at its own time),
  [ADR-0244](ADR-0244-os-version-floors-and-gates.md) (`Since<F>` gates for Android API 29 and 30),
  [ADR-0152](ADR-0152-capability-seam-revised.md) (the reasons a capability is missing)

## Context and problem statement

A window published one inset kind, `WindowState::safe_area`, which the runtime wrote to
`MediaQueryData::view_padding` (`PendingWindowState::apply`,
`crates/flui-runtime/src/owner/presentation_dispatch.rs`). Three things were missing:

| Gap | What failed |
|---|---|
| Keyboard occlusion | No host reported the soft keyboard and nothing wrote `MediaQueryData::view_insets`, so `Scaffold`'s `resize_to_avoid_bottom_inset` was dead in production: a focused field at the bottom stayed under the keyboard. |
| Keyboard motion | Even with a value, `PendingWindowState` applies the newest one once, so content would jump in one frame while the OS animates the keyboard over about 250 ms. |
| System gesture insets | No host read Android's system-gesture insets, so an app could not tell that a horizontal drag at a screen edge would be taken by back or home. |

`MediaQueryData` already had the fields the widgets read (`view_padding`, `view_insets`, the
derived `padding()`, `remove_bottom_view_inset`). What was missing was the contract between a host
and the runtime.

## Decision drivers

- One value per window, published in the snapshot like every other window fact (ADR-0254), so
  ordering, revisions, recording and replay need no new channel.
- A frame lays out with the inset at that frame's time: the keyboard's motion is sampled on the
  frame clock, never by a timer or by the arrival of a host event.
- A host that cannot report the keyboard says so through its capabilities, with a reason, so a
  scenario fails loudly as FLUI's gap instead of reading a silent zero.
- Room to grow: a new inset kind (a display feature, a caption area) is a field of the value, not a
  new observation.
- Logical geometry only, finite and non-negative: non-finite geometry is never published.

## Considered options

1. Keep `safe_area` and add `WindowObservation::Keyboard(EdgeInsets)` and
   `WindowObservation::GestureInsets(EdgeInsets)`, each applied like the safe area.
2. One `WindowInsets` value in the snapshot; the host reports a keyboard sample per frame
   (Android's `onProgress`), and the runtime applies the newest.
3. One `WindowInsets` value in the snapshot; the keyboard carries its target and, while it moves,
   a motion (from, start, duration, curve) the runtime samples at each frame's time.

## Decision

Option 3.

### §1 The value, in `flui-platform`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WindowInsets { /* safe_area, keyboard, system_gestures: private */ }

impl WindowInsets {
    pub const ZERO: Self;
    pub fn try_new(
        safe_area: EdgeInsets,
        keyboard: KeyboardInset,
        system_gestures: EdgeInsets,
    ) -> Result<Self, InvalidInsets>;
    pub fn try_with_safe_area(self, safe_area: EdgeInsets) -> Result<Self, InvalidInsets>;
    pub const fn with_keyboard(self, keyboard: KeyboardInset) -> Self;
    pub fn try_with_system_gestures(self, gestures: EdgeInsets) -> Result<Self, InvalidInsets>;
    pub const fn safe_area(&self) -> EdgeInsets;
    pub const fn keyboard(&self) -> KeyboardInset;
    pub const fn system_gestures(&self) -> EdgeInsets;
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyboardInset { /* target, motion: Option<InsetMotion> */ }

impl KeyboardInset {
    pub const HIDDEN: Self;
    pub fn try_at_rest(target: EdgeInsets) -> Result<Self, InvalidInsets>;
    pub fn try_moving(target: EdgeInsets, motion: InsetMotion) -> Result<Self, InvalidInsets>;
    pub const fn settled(self) -> Self;
    pub fn settles_at(&self) -> Option<EventTime>;
    /// `from` before the motion starts, the curve's value while it runs,
    /// `target` from its end on and at rest.
    pub fn at(&self, time: EventTime) -> EdgeInsets;
}

impl InsetMotion {
    pub fn try_new(
        from: EdgeInsets,
        start: EventTime,
        duration: Duration,
        curve: InsetCurve,
    ) -> Result<Self, InvalidInsets>;
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum InsetCurve {
    Linear,
    CubicBezier { x1: f64, y1: f64, x2: f64, y2: f64 },
    /// UIKit's keyboard curve, which `UIKeyboardAnimationCurveUserInfoKey`
    /// reports as 7, a value `UIViewAnimationCurve` does not name.
    SystemKeyboard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InvalidInsets { NonFinite, Negative, ZeroDuration, InvalidCurve }
```

The fields are private and every constructor validates, so an inset that is not finite or is
negative, a zero duration and a Bézier whose control times leave `[0, 1]` cannot be built.
`WindowState::safe_area` becomes `WindowState::insets`, and `WindowObservation::SafeArea` becomes
`WindowObservation::Insets(WindowInsets)`.

The system gesture regions are where a system gesture (back, home, the notification shade) takes
precedence over the app's own drag. An app can still claim part of one through its input regions
(Android's exclusion rects, `preferredScreenEdgesDeferringSystemGestures`), which this value does
not reflect.

`SystemKeyboard` is its own variant, not a Bézier a host computes, because UIKit's curve 7 is a
private spring that no public API names. FLUI evaluates it as
`cubic-bezier(0.380, 0.700, 0.125, 1.000)` (`InsetCurve::SYSTEM_KEYBOARD_BEZIER`), the measurement
the iOS community uses for it (for example in `CAMediaTimingFunction` replacements that track the
keyboard). The constant is checked against a device recording before the iOS producer ships;
changing it then changes no type.

### §2 Capabilities say what a window reports

`WindowCapabilities` gains two report capabilities beside `reports_presentation`, each with its
absence:

- `reports_keyboard_inset`, cleared by `without_keyboard_inset(absence)`, read by
  `keyboard_inset()`;
- `reports_system_gestures`, cleared by `without_system_gestures(absence)`, read by
  `system_gestures()`.

| Host | keyboard | system gestures |
|---|---|---|
| Android | `Native` | `Native` from API 29, `NotOnThisDevice` below |
| iOS | `NotYetOnThisHost` | `NotOnThisPlatform` (UIKit names deferred edges, not insets) |
| Web | `NotYetOnThisHost` | `NotOnThisPlatform` |
| Win32 | `NotYetOnThisHost` (a tablet's touch keyboard, `InputPane`) | `NotOnThisPlatform` |
| winit shells | `NotYetOnThisHost` | `NotOnThisPlatform` |
| macOS, X11, Wayland | `NotOnThisPlatform` | `NotOnThisPlatform` |

A zero inset from a window that reports is a fact; a zero from one that does not is unknown, and a
scenario checks the capability first. The journal records both capabilities; a recording made
before them reads them as `NotOnThisPlatform`.

### §3 The frame samples the keyboard

`PendingWindowState` coalesces the newest `WindowInsets` like any other value: a new motion
replaces the old one, which is right, since the OS has re-targeted. Applying it writes
`view_padding = safe_area()` and `system_gesture_insets = system_gestures()` into the
presentation's `MediaQueryData`, and hands the keyboard to the presentation. A keyboard at rest
writes `view_insets = target()` at once.

A moving keyboard is sampled in the presentation's sampling stage, before build, at the vsync of
the frame being built: `view_insets = keyboard.at(vsync)`. While the motion has not ended, the stage
asks for the next frame; at or after its end the keyboard is kept settled and no frame is asked for.
`MediaQueryData` gains `system_gesture_insets` and `MediaQuery::system_gesture_insets_of`.

### §4 What each host publishes

| Host | safe area | keyboard | system gestures |
|---|---|---|---|
| Android | `systemBars() \| displayCutout()` as today | `Type.ime()`; API 30 and later: `WindowInsetsAnimation.Callback` publishes a motion at `onStart` (bounds, `getDurationMillis`, the interpolator mapped to `InsetCurve`) and re-anchors it at each `onProgress`; below API 30 the target alone | `systemGestures()` (API 29 and later) |
| iOS | `safeAreaInsets` as today | `UIKeyboardWillChangeFrameNotification`: the end frame converted to the view, the duration, the curve (7 is `SystemKeyboard`) | none |
| Web | `env(safe-area-inset-*)` with `viewport-fit=cover` | `navigator.virtualKeyboard` with `overlaysContent` and `geometrychange` where the browser has it, else the `visualViewport` resize (`innerHeight - (visualViewport.height + offsetTop)`); the target alone, and the capability is `Native` or `Emulated` by which the browser offers | none |
| Desktop | as today | none | none |
| Headless | `HeadlessHost::set_window_insets` sets any value; `HeadlessHost::move_keyboard` starts a motion now, from where the keyboard is | same | same |

### §5 Tests script the motion

`HeadlessHost::move_keyboard(window, target, duration, curve)` returns the `KeyboardInset` it
reported, so a test reads the expected occlusion of each frame with `at(t)` on the virtual clock,
and `time_to_next_frame` shows the frame requests ending with the motion.

### Consequences

- Good: one value, one observation, one recording variant; replay reproduces a keyboard motion
  exactly, since the motion is data on the event timeline.
- Good: content follows the keyboard frame by frame on iOS and Android 11 and later, and
  `Scaffold`'s `resize_to_avoid_bottom_inset` works once the producers land.
- Good: an edge slider can read `system_gesture_insets` and keep its handles out of back and home.
- Bad: a breaking change to `WindowState`, `WindowObservation` and the input-recording format
  (pre-1.0).
- Bad: Android below API 30 and the Web jump to the target in one frame: they have no motion data.
- Neutral: the runtime asks for frames during a motion even when nothing else changes, which is the
  cost of following the OS's animation.

### Confirmation

- `flui-platform` table test `window_insets`: `at()` before the start, at the start, mid-curve, at
  the end and after it, for each curve; non-finite and negative edges, a zero duration and an
  invalid curve refused.
- `flui-testing` test `a_keyboard_motion_lays_out_each_frame_at_its_time`: a 250 ms motion builds
  each frame with `at(vsync)`, the last at the target, and asks for no frame after it;
  `reported_insets_reach_their_media_query_fields` for the three fields at rest.
- The recording vocabulary row for `WindowObservation::Insets` round-trips a moving keyboard; the
  native journal's window-state row carries the insets and both capabilities.
- Android, iOS and Web producers each add a conformance row and run on a device (inventory);
  `docs/platform-parity.md` moves only after those runs.

## Pros and cons of the options

### 1. An observation per kind

- Good: the smallest diff.
- Bad: three channels for one fact; each new kind is a new observation, recording variant and apply
  branch; there is no place for the motion, so content still jumps.

### 2. A host-reported sample per frame

- Good: exact on Android, where the OS drives `onProgress`.
- Bad: iOS gives only target, duration and curve, so its host would need its own frame timer, a
  second clock; samples arrive on the host's schedule rather than the frame's, coalescing drops
  some, and replay depends on host timing.

### 3. Target and motion, sampled on the frame clock (chosen)

- Good: one shape for both mobile systems; the frame decides the value, as ADR-0083 requires;
  deterministic in tests and replay.
- Bad: Android's interpolator is mapped to a curve (its default IME interpolator is a Bézier); a
  custom one is approximated, and re-anchoring at each `onProgress` corrects the drift.
