### Added

- `flui_foundation::{SlotKey, SlotIndex, SlotSet}`: side tables addressed by a generational key's
  slot, compared by the full key, whose `clear` costs the entries they hold.

### Fixed

- A frame's cost no longer grows with the size of the tree after a mount: the render pipeline's
  dirty queues and the build owner's per-frame set of built elements were hash tables whose
  `clear` walked every bucket grown during the mount. A label change in a tree of 900 000 elements
  took 157 µs and now takes 22 µs, the same as at 1 800 elements.
