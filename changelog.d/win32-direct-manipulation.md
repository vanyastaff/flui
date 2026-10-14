### Added

- **Precision touchpad on Windows** (`flui-platform`): each Win32 window owns a DirectManipulation
  viewport, so a two-finger pan arrives as phased `Scroll` with `Momentum*` inertia and a pinch as
  `PanZoom`. `input_availability::windows` takes whether the viewport initialised and reports
  `precise_scroll` and `trackpad_gestures` as `Native`, otherwise `Unavailable`.
