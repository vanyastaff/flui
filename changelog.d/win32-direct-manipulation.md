### Added

- **Precision touchpad on Windows** (`flui-native`): each Win32 window owns a DirectManipulation
  viewport, so a two-finger pan arrives as phased `Scroll` with `Momentum*` inertia and a pinch as
  `PanZoom`. The window pumps its viewport itself (before each frame, and on a pacing timer that
  also runs inside menu and drag loops), requests its own frames while a manipulation runs or
  coasts, and cancels what is open when the modal size-and-move loop starts, accepting contacts
  again when it ends. `input_availability::windows` takes whether the viewport initialised and
  reports `precise_scroll` and `trackpad_gestures` as `Native`, otherwise `Unavailable`.
