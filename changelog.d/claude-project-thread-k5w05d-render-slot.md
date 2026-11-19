### Changed

- A render node's slot in `flui-rendering`'s render tree is 216 bytes in a debug build, down from
  336: a sliver keeps its last geometry and constraints behind one allocation made on its first
  layout instead of sizing every node's slot, a box keeps their presence in two bits, and the
  local invalidation handle lives in the attached state. `Protocol` gains the associated type
  `LayoutRecord` (the trait is sealed). A `RawButton` over a `Text` mounts in 4146 bytes, down
  from 4269, with no allocation added to any control's mount.
