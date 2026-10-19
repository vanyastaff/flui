### Fixed

- **Frame-aligned haptics played with frames nobody saw** (`flui-runtime`): a
  `HapticTiming::FramePresented` request now plays only with a frame the sink reports
  `Presented`; a frame the surface could not show (`NotShown`) or one with no damage
  (`NoPresent`, the screen keeps the previous frame) leaves it to a later presented frame or
  its deadline, where it resolves `Suppressed`.
