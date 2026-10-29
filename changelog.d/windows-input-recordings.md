### Added

- **`flui-native`**: native recordings journal mouse wheel turns and precision-touchpad viewport
  calls (DirectManipulation callbacks and abandonment) under `native-capture`, so a Windows
  session with a wheel or a touchpad replays through the shared decoders on any host
  ([ADR-0215](/docs/adr/ADR-0215-input-recording-and-backend-conformance.md) §4). Golden
  recordings of a touch and a pen injected through `InjectSyntheticPointerInput`, and of mouse
  buttons and both wheels sent through `SendInput`, are checked in and re-recorded by
  `FLUI_NATIVE_RECORDING_BLESS` (§8).

### Fixed

- **`flui-native`**: on Windows a mouse is one pointer: its `WM_POINTER` presses share the
  pointer identity of its hover and its wheel scrolls instead of arriving as a second pointer.
