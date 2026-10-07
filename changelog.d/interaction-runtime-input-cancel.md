### Fixed

- **Held pointer input on focus loss and pause** (`flui-runtime`): a touch or click that lands
  before a window's first frame is presented, and whose window then loses focus, is hidden or is
  paused before the release, no longer replays its Down at the next commit and leaves a gesture
  route open until the next press. The open sequence is dropped with the cancellation; a tap that
  already completed still replays.
