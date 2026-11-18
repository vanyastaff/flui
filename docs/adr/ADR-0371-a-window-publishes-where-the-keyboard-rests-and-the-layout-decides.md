# ADR-0371: A window publishes where the keyboard rests, and the layout decides

- **Status:** Accepted (2026-11-18)
- **Date:** 2026-11-18
- **Supersedes:** [ADR-0277](ADR-0277-a-window-publishes-its-insets-and-the-keyboards-motion.md)
  §1 for `KeyboardInset` only (its target becomes a placement), §3 for what applying a keyboard
  writes, the iOS keyboard cell of §4 and `HeadlessHost::move_keyboard`'s target in §5
- **Related:** [ADR-0255](ADR-0255-mobile-and-web-hosts.md) (where each host reads its insets),
  [ADR-0095](ADR-0095-agent-protocol-schema-crate.md) §1 (the protocol's revision rule)

## Context and problem statement

ADR-0277 made the keyboard one value: the edges it covers once its motion ends, and the motion.
Every producer turned the keyboard's frame into a bottom inset, from the keyboard's top to the
view's bottom wherever the two overlapped. An iPad has keyboards that are not docked: the floating
keyboard, the undocked and split keyboards, and a shortcuts bar dragged up. One of those in the
middle of a view became an inset covering everything below its top, so a `Scaffold` that resizes
for the keyboard shrank its body to the strip above a small panel.

UIKit also sends notices the producer read wrongly or not at all: a `WillChangeFrame` with a zero
end frame while the user drags a floating keyboard, which read as hidden, so the inset flickered; a
hide whose `WillChangeFrame` end frame is still partly on screen, with no `WillHide` observer to
say it is a hide, which left a stale inset; and a `WillShow` with a zero end frame for a shortcuts
bar the user dropped floating, with no `WillShow` observer, which left the docked bar's inset.

The two references disagree on what a floating keyboard means. Flutter's
`KeyboardInsetManager` classifies a keyboard whose bottom lies above the screen's as floating and
gives it no inset; Raven covers what lies below an undocked keyboard on purpose. Neither answer is
right for every layout: a chat composer wants to stay where it is under a floating panel it can
move out from under, a form wants to scroll its focused field clear of it.

## Decision drivers

- The host tells the kinds of keyboard apart, since only it reads the platform's frames; the
  layout decides what each means for it.
- A docked keyboard keeps its frame-by-frame motion (ADR-0277 §3).
- A host that cannot tell a floating keyboard from a docked one publishes what it knows,
  faithfully: docked or hidden.
- A recording made before the change still reads.

## Considered options

1. Keep the inset and classify a floating keyboard as no keyboard (Flutter's behaviour).
2. Keep the inset and cover what lies below a floating keyboard (Raven's behaviour).
3. Publish the placement, `Hidden`, `Docked { inset }` or `Floating { frame }`; the inset moves
   along the motion, and the floating frame reaches the `MediaQuery` beside it.

## Decision

Option 3.

### §1 The value, in `flui-platform`

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum KeyboardPlacement {
    Hidden,
    Docked { inset: EdgeInsets },
    Floating { frame: Rect<f64> },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyboardInset { /* placement, motion: Option<InsetMotion> */ }

impl KeyboardInset {
    pub const HIDDEN: Self;
    pub fn try_at_rest(placement: KeyboardPlacement) -> Result<Self, InvalidInsets>;
    pub fn try_moving(placement: KeyboardPlacement, motion: InsetMotion)
        -> Result<Self, InvalidInsets>;
    pub const fn settled(self) -> Self;
    pub const fn placement(&self) -> KeyboardPlacement;
    /// The docked inset; zero hidden or floating.
    pub const fn target(&self) -> EdgeInsets;
    /// The floating keyboard's frame; `None` docked or hidden.
    pub const fn floating_frame(&self) -> Option<Rect<f64>>;
    pub const fn motion(&self) -> Option<InsetMotion>;
    pub fn settles_at(&self) -> Option<EventTime>;
    pub fn at(&self, time: EventTime) -> EdgeInsets;
}
```

The constructors refuse a docked inset that is not finite or is negative, and a floating frame
that is not finite (`InvalidInsets::NonFinite`) or has no area (`InvalidInsets::EmptyFrame`). A
docked placement that covers no edge is held as `Hidden`, so one placement has one spelling. The
motion moves the inset: toward the docked inset, or toward none for a keyboard that hides or comes
to float.

### §2 Applying it

Applying a keyboard writes `view_insets = target()` at rest, and a moving one is sampled at each
frame's vsync as ADR-0277 §3 says. `MediaQueryData::floating_keyboard: Option<Rect>` is
`floating_frame()`, written at once when the keyboard is reported; `None` docked or hidden.

### §3 What each host publishes

| Host | Placement |
|---|---|
| iOS | `UIKeyboardWillChangeFrameNotification`, `UIKeyboardWillShowNotification` and `UIKeyboardWillHideNotification`. The end frame of a frame change or a show, converted into the view at each metrics sample: hidden off the view or with no area, floating when its bottom, rounded to the point, lies above the view's, docked otherwise. A hide is hidden whatever its frame. A notice that says nothing of where the keyboard ends up keeps the one that stood: a frame change or a show with no end frame, a frame change with UIKit's zero frame (the user is dragging a floating keyboard), a show that repeats the end frame of the frame change standing. A show with the zero frame is a shortcuts bar dropped floating, which UIKit does not place: hidden. |
| Web | The same rule over `navigator.virtualKeyboard`'s rectangle or the layout viewport below the visual viewport, in the window's logical pixels; a docked keyboard reaches the layout viewport's bottom, or the canvas's where the canvas ends above it. |
| Android | `Docked` from `Type.ime()` as before: Android does not report a floating input method in its insets. |
| Headless | `HeadlessHost::set_window_insets` sets any placement; `HeadlessHost::move_keyboard(window, placement, duration, curve)` moves to one. |

The rule that reads a frame is one function for the hosts that have a frame
(`flui_native::window::keyboard::placement`), so iOS and the Web cannot come to disagree on it.

### §4 The recordings

The native journal's and the input recording's keyboard gain an optional `floating` frame
(x, y, width, height) beside `target` and `motion`: protocol 0.27, additive. A reader refuses a
floating keyboard that names an inset as well; a recording without the field reads as docked or
hidden.

### Consequences

- Good: a floating keyboard no longer shrinks a resizing layout, and a layout that wants to keep
  something clear of it reads its frame.
- Good: the dragged keyboard no longer flickers the inset, and a hide is a hide.
- Bad: a breaking change to `KeyboardInset`'s constructors and to
  `HeadlessHost::move_keyboard` (pre-1.0).
- Bad: a floating keyboard that UIKit does not place (a shortcuts bar dropped floating) is
  published as hidden: the layout learns that nothing is docked, not where the bar is.
- Neutral: a floating keyboard over a window whose bottom it straddles (a short Stage Manager
  window) reads as docked, since the iOS rule compares bottoms with the view's: Flutter's
  screen-bottom rule reads an iPad Slide Over keyboard as floating instead and needs an adjustment
  for it.
- Neutral: a keyboard another application shows beside the window in Split View is published
  like the window's own, since it covers the window all the same; Flutter ignores it.

### Confirmation

- `flui-native` `window::keyboard` tests `a_frame_names_where_the_keyboard_rests` (a frame whose
  bottom is above the view's is floating, not a docked inset) and
  `a_notice_keeps_or_replaces_the_keyboard` (a zero frame keeps the inset; a hide is hidden with
  any frame; a show with the zero frame leaves no inset).
- `flui-native` wasm32 test `web_window_has_the_virtual_keyboard_overlay_its_page`: a rectangle
  that ends above the layout viewport's bottom is published floating, in logical pixels.
- `flui-platform` `window_insets` row `a_floating_keyboard_has_no_inset`; `flui-testing`
  `a_floating_keyboard_reaches_the_media_query_as_a_frame`.
- The recording vocabulary row and the journal's window-state row round-trip a keyboard; the
  `wire_schema` test publishes protocol 0.27.
- The docked, floating, dragged and hidden keyboards run on the iOS Simulator
  (`docs/platform-parity.md`, "Soft-keyboard insets").

## Pros and cons of the options

### 1. A floating keyboard is no keyboard

- Good: no new type; what Flutter ships.
- Bad: a field under the panel stays under it, and the layout cannot know.

### 2. A floating keyboard covers what lies below it

- Good: no new type; a focused field is never under the panel.
- Bad: a small panel in the middle shrinks a resizing layout to the strip above it.

### 3. The placement (chosen)

- Good: the host says what it knows, and each layout chooses between 1 and 2 for itself.
- Bad: one more field for a layout to consider, and a new constructor shape for every producer.
