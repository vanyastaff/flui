### Added

- `FrameRun::try_pump` in flui-rendering's `testing` harness returns the frame's `RenderError` instead of panicking.

### Fixed

- Re-assembling marked semantics subtrees walks their anchors in render preorder, so a frame that refuses reports the same node as assembling the whole tree, instead of one picked by hash order.
