### Changed

- `FrameSnapshot` names its frame's UI runtime: the `presentation: PresentationId` field is now
  `address: PresentationAddress`, and `FrameClock::record_frame` and its variants take that
  address. The `flui.frame` telemetry event carries `ui_runtime_id`, and the devtools timeline
  names each frame by its UI runtime and presentation, so frames of two UI runtimes on one owner
  thread no longer collide in one trace (ADR-0266 §3).
