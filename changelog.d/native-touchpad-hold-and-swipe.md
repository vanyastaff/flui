### Added

- A swipe of three fingers or more on a touchpad is `PointerEvent::TouchpadSwipe`: its finger count and its travel since the start (`SwipeOffset`, logical pixels), from Wayland (`zwp_pointer_gesture_swipe_v1`) and X11 (XInput 2.4). It is a gesture, never a scroll: `SwipeGestureRecognizer` takes it as a swipe, reporting `PointerKind::Trackpad`, and no scrollable moves. A recognizer of your own admits one through `GestureRecognizer::add_touchpad_swipe`.
- Fingers resting on a touchpad are `PointerEvent::TouchpadHold` (`Begin`, `End`, `Cancelled`), from Windows' precision touchpad, macOS and Wayland (`zwp_pointer_gesture_hold_v1`); a `Scrollable` stops its fling when fingers rest, on Windows and Wayland too now.
- `InputCapabilities::touchpad_hold` and `touchpad_swipe` say whether a window receives each.
- `TrackpadScript::lift`, `swipe_start`, `swipe_to`, `swipe_end` and `swipe_cancel` (testing); a script whose hold or swipe does not end is refused.

### Changed

- Input recordings are format version 5 (`touchpad_hold` and `touchpad_swipe` rows); version 4 recordings are refused. The agent protocol is 0.19, which a 0.18 client cannot speak to.

### Removed

- `PointerEvent::ScrollInertiaCancel`: a hold's `Begin` is the fingers that come to rest.
