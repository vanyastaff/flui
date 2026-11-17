# ADR-0358: A touchpad hold stops what coasts, and a touchpad swipe is a gesture

- **Status:** Accepted (2026-11-17)
- **Date:** 2026-11-17
- **Supersedes:** [ADR-0143](ADR-0143-flui-owned-input-event-vocabulary.md) §1 for
  `ScrollInertiaCancel` among the pointer variants and §2 for it as the finger that stops the
  platform's momentum
- **Related:** [ADR-0346](ADR-0346-a-key-down-says-its-kind-a-dead-key-its-accent-and-a-scroll-its-direction.md)
  (the input vocabulary this record extends), [ADR-0342](ADR-0342-a-value-added-to-a-closed-union-breaks-the-agent-protocol.md)
  (a variant added to a closed union is a protocol break), [ADR-0208](ADR-0208-gesture-ownership-table-and-composition.md)
  (the arena a swipe competes in), [ADR-0213](ADR-0213-input-capabilities.md) (the capabilities that
  say where each reaches a window), [ADR-0215](ADR-0215-input-recording-and-backend-conformance.md)
  (the recordings that mirror the vocabulary), [ADR-0217](ADR-0217-interaction-module-layers-and-pointer-assembly.md)
  §4 (a two-finger pan is a phased scroll)

## Context and problem statement

Two things a touchpad reports had no shape in the input vocabulary:

| Fact | What happened to it |
|------|---------------------|
| Fingers coming to rest | AppKit's `NSEventPhaseMayBegin` became `PointerEvent::ScrollInertiaCancel(PointerSignal)`, one signal with no end. Wayland's `zwp_pointer_gesture_hold_v1` was not bound ("swipes and holds are the compositor's"), DirectManipulation's viewport running before a slop published nothing, so a fling the framework drove went on under fingers that rested on Wayland and Windows. |
| A swipe of three fingers or more | Not bound on Wayland (`zwp_pointer_gesture_swipe_v1`) or X11 (XInput 2.4 `XI_GestureSwipe*`): "swipes are the window manager's", though a compositor or window manager that binds none of them hands them to the window. |

GTK 4 stops kinetic scrolling at a hold's begin; Raven stops what coasts on a hold and maps a
swipe to the mouse's back and forward buttons; Flutter has only the inertia cancel signal and no
swipe. The question: what shape does each fact take, and how does a swipe reach the code that
acts on it without becoming a scroll?

## Decision drivers

- One fact, one form: a hold that replaces the inertia cancel leaves no second "stop the fling"
  event alive.
- A sequence that begins ends (ADR-0143): a hold has an end, as a contact has.
- A swipe is a gesture, so it competes for its consumer as gestures do, and no scrollable reads it.
- A value carries what the platform reports, in logical pixels; where a host cannot report a
  fact, its capabilities say so.
- The recording and the agent protocol mirror the vocabulary one to one (ADR-0215).

## Considered options

For the hold:

1. **Keep `ScrollInertiaCancel`** and have Wayland and Windows produce it at a hold's begin. One
   signal, no end, so a hold's lift and a hold that turned into a scroll stay unsaid.
2. **`PointerEvent::TouchpadHold` with `Begin`, `End`, `Cancelled`**, replacing
   `ScrollInertiaCancel`. AppKit's `MayBegin`, its cancel and its `Began` map onto the three, as
   Wayland's hold begin and end (`cancelled`) and DirectManipulation's `Running` before a slop do.

For the swipe:

- (a) **A `PanZoom` with a finger count.** `PanZoomTransform` has no
  translation (ADR-0217 §4), and every pinch consumer would meet swipes.
- (b) **A phased scroll.** A scrollable would scroll on it, which is the defect.
- (c) **`PointerEvent::TouchpadSwipe` with its finger count and a cumulative travel**, delivered by
  the binding along the path its start took, its arena contest keyed by the swipe's pointer, and
  offered to recognizers through `GestureRecognizer::add_touchpad_swipe`.
- (d) **A claim target of its own** (as `SmartZoomTarget`), a registry, a render-object field and a
  listener callback across five crates, for a gesture the arena already arbitrates.

## Decision

Chosen: option 2 and (c), because each states its fact in one shape and reuses the arbitration the
framework already has.

### 1. A hold

```rust
pub enum TouchpadHoldPhase { Begin, End, Cancelled }
pub struct TouchpadHoldEvent { pointer: PointerSource, pub time: EventTime,
    pub position: PointerPosition, pub phase: TouchpadHoldPhase, pub modifiers: Modifiers }
PointerEvent::TouchpadHold(TouchpadHoldEvent)
```

`ScrollInertiaCancel` leaves the vocabulary, the binding, the listener and the recording. A hold is
delivered as a signal where the pointer rests (`Listener::on_pointer_signal`). Every ballistic
motion under the pointer stops at `Begin`, as a press stops it: a `Scrollable`'s fling and an
`InteractiveViewer`'s inertia (pagers, carousels and wheels scroll through a `Scrollable`). A hold
that ends or is cancelled, because the fingers lifted or started a gesture, leaves the motion
stopped, as GTK 4 does; the gesture that follows drives it anew. An `AnchoredDrag`'s settle to an
anchor (sheets, swipe rows) is not inertia and is not stopped. Windows begins a hold when its DirectManipulation viewport runs and
cancels it as the pan or pinch passes its slop; macOS at a scroll's `MayBegin`, ended by AppKit's
cancel of a scroll that never began and cancelled by the scroll's `Began`; Wayland binds
`zwp_pointer_gestures_v1` at version 3 for `zwp_pointer_gesture_hold_v1`.

### 2. A swipe

```rust
pub struct SwipeOffset(Offset<f64>);            // finite; travel since the start, logical pixels
pub enum TouchpadSwipePhase { Start, Update(SwipeOffset), End, Cancelled }
pub struct TouchpadSwipeEvent { pointer: PointerSource, pub time: EventTime,
    pub position: PointerPosition, pub fingers: NonZeroU8, pub phase: TouchpadSwipePhase,
    pub modifiers: Modifiers }
PointerEvent::TouchpadSwipe(TouchpadSwipeEvent)
```

The travel is cumulative, as `PanZoomTransform` is; the shared assembly sums a host's steps
(`ReadingAction::Swipe`). `position` is where the pointer rests, which no swipe moves. The binding
hit-tests a swipe's `Start`, delivers every phase along that path (`TouchpadSwipes`), closes the
arena keyed by the swipe's pointer after the start, and at `End` or `Cancelled` detaches that
contest before any user code runs, then sweeps or abandons the detached generation, so a contest a
callback opens for the same pointer is never the one settled. It cancels an open swipe itself on
focus loss, its device's removal or a newer start of its pointer. A host mints a pointer for each
swipe, and a start over a swipe whose end was lost cancels that one first (the shared assembly).
`RecognizerSet` offers a start to `GestureRecognizer::add_touchpad_swipe` of every recognizer that
admits a trackpad (a default method that admits nothing), so no contact recognizer reads a swipe;
`SwipeGestureRecognizer` with `SwipeStart::Anywhere` follows the travel as a contact's motion and
its lift as a release, and reports `SwipeDetails::kind` as a trackpad. A `SwipeStart::FromEdge`
recognizer takes none: a touchpad swipe starts where the cursor rests, not from an edge. Wayland binds `zwp_pointer_gesture_swipe_v1`;
X11 selects XInput 2.4's swipe events and reads the finger count from their `detail`.

### 3. Where each reaches a window

`InputCapabilities::touchpad_hold` and `touchpad_swipe`: Windows with a DirectManipulation viewport,
macOS and Wayland at gestures version 3 deliver holds; Wayland and X11 at XInput 2.4 deliver
swipes. The rest are `Unavailable` by the platform: X11 has no hold; Windows keeps three- and
four-finger swipes for the shell; AppKit reports a swipe only as a finished direction
(`swipeWithEvent:`); Android's navigation takes multi-finger swipes and reports a travel only as a
fraction of the touchpad; UIKit and the Web report neither.

### 4. The recording and the protocol follow

The input recording carries `touchpad_hold` and `touchpad_swipe` rows and the two capabilities.
A version 4 recording may hold a `scroll_inertia_cancel` row, which no reader has any more, so the
format's version moves and the older one is refused by its version (`RecordingError::Version`), never
read halfway. The two capabilities default to `unavailable` when a writer leaves them out only
because the protocol refuses a field made required (ADR-0342); no older recording reaches that
default. The native recording's reading action gains `hold` and `swipe`. Protocol 0.21
is a break (ADR-0342 §1 for the added variants) and lists `scroll_inertia_cancel` in `BREAKING`'s
`removed`.

### Consequences

- Good, because a fling or a viewer's inertia stops when fingers rest, on every host that reports
  resting fingers, and the end of a hold is said.
- Good, because a swipe competes for its consumer in the arena, so a horizontal and a vertical
  swipe recognizer nested in one another split swipes by axis, and a scrollable never moves.
- Bad, because a 0.20 agent client is refused by a 0.21 application, and version 4 recordings no
  longer read.
- Neutral, because no host's swipe or hold has run on a touchpad; each is held at its seam
  (`crates/flui-native/platform-parity.toml`).

### Confirmation

- Tests: `a_touchpad_swipe_is_a_gesture_and_no_scroll` and
  `resting_fingers_catch_a_fling_and_leave_wheel_easing` (flui-widgets, through the binding);
  `a_manipulation_inside_the_slop_is_a_hold`, `resting_fingers_begin_a_hold_that_a_lift_ends_and_a_scroll_cancels`,
  `a_swipe_is_one_gesture_where_the_cursor_rests`, `a_hold_rests_until_it_lifts_or_a_gesture_starts`,
  `a_touchpad_swipe_is_one_gesture_of_its_fingers`, `a_swipe_sums_its_steps_into_its_travel`,
  `a_hold_and_a_swipe_end_with_their_device` and
  `every_backend_says_whether_a_touchpad_hold_and_swipe_reach_it` (flui-native);
  `every_vocabulary_variant_round_trips_through_the_recording_dto` (flui-testing);
  `every_published_schema_is_additive_to_the_next` (flui-protocol).
- The compiler: `TouchpadSwipePhase` and `TouchpadHoldPhase` are closed, so every consumer that
  matches them names each phase.
