### Added

- **Input regions** (`flui-platform-api`, `flui-rendering`, `flui-objects`, `flui-widgets`,
  `flui-native`): `SystemGestureExclusion` keeps the OS's edge gestures out of its child's box.
  Render objects (boxes and slivers) report regions through `RenderObject::describe_input_regions`
  (`RenderInputRegion`); `PipelineOwner::input_regions` maps them into the window through paint
  transforms, follower offsets and clips, and the runtime hands each composited frame's set to the new
  `PlatformWindow::set_input_regions`. Backends project with `ProjectedInputRegions` and update
  the OS only when the device pixels, scale or display changed: Android applies
  `setSystemGestureExclusionRects` (API 29+), iOS defers the screen edges the regions reach, and
  `InputCapabilities::input_regions` reports `Native` there.
