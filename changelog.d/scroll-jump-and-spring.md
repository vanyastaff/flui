### Changed

- **Scroll requests during a pass wait for it** (`flui-widgets`): a `ScrollController::jump_to`,
  `animate_to` or reveal made while a pass of the nested scroll channel runs through that
  scrollable (from a `NestedScrollConnection`, a position listener or a motion hook) is no longer
  refused. It is recorded in the controller's command slot, the latest request replacing any
  earlier one, and lands when the outermost pass returns, in the same event or frame; until then
  `pixels()` reads the old offset. A landing jump ends the fling, settle or animation the
  scrollable runs `Preempted` and is a `Programmatic` sequence of its own; a contact, a held key or
  a drop autoscroll goes on from the new offset. Requests that keep requesting are serviced eight
  at a time, and the latest waits for the next notification with a warning. User input dispatched
  from inside a pass is still refused.
- **A jump to a target that is not finite is dropped** (`flui-widgets`): it no longer stops the
  scrollable's motion or replaces a pending `animate_to`.
- **A remainder catches a springing ancestor** (`flui-widgets`): a user remainder that moves an
  ancestor scrollable stops the ancestor's own spring, fling or snap, which ends `Preempted`, and
  the ancestor moves by what it took alone. A remainder it cannot take leaves its motion running.
- **The sequence settles every scrollable it moved** (`flui-widgets`): at release, an ancestor
  scrollable a descendant's remainder moved settles under the same sequence with the release
  velocity on the axes whose latch it holds, so a pager lands on a page and a bouncing list caught
  past its edge comes back to it; a sequence that ends without a release (cancelled, the inner
  list unmounted) settles it from rest first. The inner `Scrollable` runs no fling on an axis an
  ancestor holds. This was `InteractiveViewer`'s alone and now covers every leaf.

### Fixed

- **A refused jump still stopped the fling** (`flui-widgets`): a `jump_to` from inside a
  participant stopped the scrollable's fling and dropped a pending `animate_to` while the jump
  itself was lost. It now lands when the pass returns.
- **A position listener's jump ran inside the step** (`flui-widgets`): a listener that jumped
  while a drag or fling step published the offset ran the jump's whole sequence in the middle of
  that step. It now runs after the step.
- **An `animate_to` from a participant never moved** (`flui-widgets`): it started the animation
  outside the channel, whose frames the scrollable then ignored. It now runs.
- **A spring and a remainder moved an ancestor twice** (`flui-widgets`): an ancestor springing back
  past its edge moved by the remainder and by its spring at once.
- **An ancestor moved by a `Scrollable`'s drag stayed where the drag left it** (`flui-widgets`):
  a pager or bouncing list moved by an inner list's remainder was not settled at release, and a
  rebuild that retired it mid-drag cut it off from the drag. Both now settle, and a jump still
  waiting when its scrollable unmounts is written instead of dropped.
