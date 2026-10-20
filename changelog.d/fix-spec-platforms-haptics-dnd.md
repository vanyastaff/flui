### Fixed

- **A drop was felt before it was seen** (`flui-widgets`): `Draggable` plays its `Drop` haptic
  with the frame that shows the drop (`HapticTiming::FramePresented`), as its `DragLift` already
  did, instead of at once inside the release.
