# ADR-0372: A window publishes where the keyboard rests, and the layout decides

- **Status:** Accepted (2026-11-18)
- **Date:** 2026-11-18
- **Supersedes:** [ADR-0277](ADR-0277-a-window-publishes-its-insets-and-the-keyboards-motion.md)
  §1 for `KeyboardInset` only (it is `SoftKeyboard`, and its target becomes a placement), §3 for
  what applying a keyboard writes, the iOS and Web keyboard cells of §4 and
  `HeadlessHost::move_keyboard`'s target in §5
- **Related:** [ADR-0255](ADR-0255-mobile-and-web-hosts.md) (where each host reads its insets),
  [ADR-0095](ADR-0095-agent-protocol-schema-crate.md) §1 (the protocol's revision rule)

## Context and problem statement

ADR-0277 made the keyboard one value: the edges it covers once its motion ends, and the motion.
Every producer turned the keyboard's frame into a bottom inset, from the keyboard's top to the
view's bottom wherever the two overlapped. An iPad has keyboards that are not docked: the floating
keyboard, the undocked and split keyboards, and a shortcuts bar dragged up. One of those in the
middle of a view became an inset covering everything below its top, so a `Scaffold` that resizes
for the keyboard shrank its body to the strip above a small panel.

UIKit also sends notices the producer read wrongly or not at all:

| Notice | What the producer made of it |
|---|---|
| `WillChangeFrame` with a zero end frame, while the user drags a floating keyboard | hidden, so the inset flickered |
| `WillHide` after a `WillChangeFrame` whose end frame is still partly on screen | not observed, so a stale inset stayed |
| `WillShow` with a zero end frame, for a shortcuts bar dropped floating | not observed, so the docked bar's inset stayed |
| `DidChangeFrame`, the only notice of where a dragged or undocked keyboard came to rest | not observed, so the frame it was dragged from stayed |

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
- A fact of the window that few layouts read costs the others nothing: a `MediaQuery` a widget
  nests copies `MediaQueryData`, and the control-cost baselines hold its size.
- A recording made before the change still reads.

## Considered options

1. Keep the inset and classify a floating keyboard as no keyboard (Flutter's behaviour).
2. Keep the inset and cover what lies below a floating keyboard (Raven's behaviour).
3. Publish the placement, `Hidden`, `Docked { inset }` or `Floating { frame }`; the inset moves
   along the motion, and the floating frame is published beside the `MediaQuery`.

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

/// The value ADR-0277 named `KeyboardInset`: it now holds keyboards that have no inset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SoftKeyboard { /* placement, motion: Option<InsetMotion> */ }

impl SoftKeyboard {
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
frame's vsync as ADR-0277 §3 says. `MediaQueryData` is unchanged. The floating frame is published
once per window, above its root, as its own inherited value, at once when the keyboard is
reported:

```rust
// flui_view::environment
pub struct FloatingKeyboard { /* frame: Option<Rect>, child */ }
impl FloatingKeyboard {
    pub fn new(frame: Option<Rect>, child: impl IntoView) -> Self;
}
impl MediaQuery {
    /// The frame of the keyboard floating over the window; a dependency on it alone.
    pub fn floating_keyboard_of(ctx: &dyn BuildContext) -> Option<Rect>;
}
```

The runtime installs the `FloatingKeyboard` (one element a window), and a test or a preview wraps
a subtree in its own.

### §3 What each host publishes

One function reads a frame for the hosts that have one,
`flui_native::window::keyboard::placement(view, frame, floor)`, in the unit the platform reports
the frame in:

| The frame | Placement |
|---|---|
| not finite, with no area, or overlapping the view nowhere | `Hidden` |
| its bottom half a unit or more above `floor` | `Floating { frame }` |
| otherwise | `Docked`, from the frame's top to the view's bottom |

The half unit is a difference of the two bottoms, never two roundings: a keyboard extension's
frame a fraction of a point short of the floor, and the last-place error of bringing a frame into
a view, both read docked.

| Host | What it publishes |
|---|---|
| iOS | The rule over the frame the notifications below leave standing, converted into the view at each metrics sample; `floor` is the view's bottom. |
| Web | The rule over `navigator.virtualKeyboard`'s rectangle or the layout viewport below the visual viewport, in CSS pixels; `floor` is the layout viewport's bottom (the smaller of the root's `clientHeight` and `innerHeight`), or the canvas's where the canvas ends above it. The placement is then scaled into the window's logical pixels. |
| Android | `Docked` from `Type.ime()` as before: Android does not report a floating input method in its insets. |
| Headless | `HeadlessHost::set_window_insets` sets any placement, `HeadlessHost::move_keyboard(window, placement, duration, curve)` moves to one, and `HeadlessWindow::with_insets` opens a window under one. |

The iOS view keeps one note of the four notifications it observes (`keyboard::Heard::kept`):

| Notification | The note kept |
|---|---|
| `WillHide`, whatever its end frame | the keyboard left its dock: hidden |
| any other with no end frame | the one that stood |
| `WillChangeFrame` or `DidChangeFrame` with UIKit's zero frame (the user is dragging) | the one that stood |
| `WillChangeFrame` with a frame | the keyboard goes to that frame, along the change it starts |
| `WillShow` or `DidChangeFrame` with the frame that stands | the one that stood: one move starts one motion |
| `DidChangeFrame` with a frame, after a `WillHide` | the hide and its motion, and the frame the keyboard came to rest at |
| `WillShow` or `DidChangeFrame` with another frame | the keyboard is at that frame |

A keyboard that left its dock is `Floating` when the frame it came to rest at floats, and
`Hidden` otherwise: UIKit announces an undocked keyboard as a hide and then says where it rests,
and a hide whose frame still lies on the bottom of the screen is a hide all the same. A `WillShow`
with the zero frame is a shortcuts bar dropped floating, which UIKit does not place: it has no
area, so its placement is `Hidden`.

### §4 The recordings

The native journal's and the input recording's keyboard gain an optional `floating` frame beside
`target` and `motion`, as the four edges the frame is kept as (left, top, right, bottom), so a
recording reads back to the bit: protocol 0.27, additive. A reader refuses a floating keyboard
that names an inset as well; a recording without the field reads as docked or hidden. The schema
keeps the name `KeyboardInset` for the recorded keyboard, since a published schema's names are not
respelled.

### Consequences

- Good: a floating keyboard no longer shrinks a resizing layout, and a layout that wants to keep
  something clear of it reads its frame.
- Good: the dragged keyboard no longer flickers the inset, it is published where it was dropped,
  and a hide is a hide.
- Good: a nested `MediaQuery` copies nothing more than before.
- Bad: a breaking change: `KeyboardInset` is `SoftKeyboard`, its constructors and
  `HeadlessHost::move_keyboard` take a placement (pre-1.0).
- Bad: the floating frame is read through its own inherited value, one more element at each
  window's root, and no widget of the framework reads it yet (`docs/gaps.md`).
- Bad: a floating keyboard that UIKit does not place (a shortcuts bar dropped floating) is
  published as hidden: the layout learns that nothing is docked, not where the bar is.
- Neutral: a floating keyboard over a window whose bottom it straddles (a short Stage Manager
  window) reads as docked, since the iOS rule compares bottoms with the view's: Flutter's
  screen-bottom rule reads an iPad Slide Over keyboard as floating instead and needs an adjustment
  for it.
- Neutral: a keyboard another application shows beside the window in Split View is published
  like the window's own, since it covers the window all the same; Flutter ignores it.

### Confirmation

- `flui-native` `window::keyboard` tests, the model on every host:
  `a_frame_names_where_the_keyboard_rests`,
  `a_docked_keyboard_a_fraction_off_its_floor_is_docked`,
  `the_notifications_heard_name_where_the_keyboard_rests` (a zero frame keeps the inset; a hide is
  hidden with any frame; a keyboard dragged, undocked or dragged off its dock rests where it was
  dropped) and `a_later_notice_of_one_move_starts_no_second_motion`.
- `flui-native` wasm32 test `web_window_has_the_virtual_keyboard_overlay_its_page`: a rectangle
  that ends above the layout viewport's bottom is published floating, in logical pixels.
- `flui-platform` `window_insets` row `a_floating_keyboard_has_no_inset`; `flui-testing`
  `a_floating_keyboard_reaches_the_media_query_as_a_frame` and
  `a_window_opened_under_a_floating_keyboard_reads_its_frame`.
- `keyboard_placements_keep_their_wire_values_and_replay` (the journal) and
  `a_floating_keyboard_recorded_with_an_inset_is_refused` (the input recording); the
  `wire_schema` test publishes protocol 0.27.
- `flui-material`'s `perf_list_tile_over_text` and `perf_navigation_bar_alone` hold the bytes of
  a control that nests a `MediaQuery` to their baselines.
- The docked, floating, dragged, undocked and hidden keyboards run on the iOS Simulator
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
- Bad: one more value for a layout to consider, and a new constructor shape for every producer.
