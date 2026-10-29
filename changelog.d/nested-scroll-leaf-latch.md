### Fixed

- **Nested scroll latch under rotation** (`flui-widgets`): the latch of a nested scroll sequence is
  kept per axis of the leaf's plane, so a pan along one axis of a turned `InteractiveViewer` or
  scrollable no longer holds both screen axes and keeps the whole contact from the pager around
  it. A wheel or trackpad sequence whose presentation swaps its gesture arena mid-sequence moves
  its idle deadline to the new arena and still ends at its latch's lapse.
