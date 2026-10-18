### Fixed

- **`flui-interaction`**: a gesture arena verdict names the exact contest it settles.
  `GestureArenaMember::accept_gesture` and `reject_gesture` take a `GestureContest` instead of a
  `PointerId`, so settling a click a double tap held no longer cancels the long press, swipe or
  force press of the same mouse's next contact, and sweeping it no longer takes away that
  contact's `Sequenced` successors. A custom member reads `contest.pointer()` where it used the
  pointer ([ADR-0230](/docs/adr/ADR-0230-arena-verdicts-name-their-contest.md)).
- **`flui-interaction`**: a drag whose OS session the host refused targets and drops in its
  window again, and no longer asks the refusing backend on every move out of the window.
- **`flui-interaction`**: an OS drop whose last hover update panics in a receiver still runs the
  drop and concludes the offer before the panic propagates.
- **`flui-interaction`**: closing a window while a drop it accepted is still fetching concludes
  the offer, so the backend releases it.
- **`flui-interaction`**: a pasted cut whose receiver panics taking it ends cancelled instead of
  staying held for a second paste; a cancel whose observer panics keeps the receiver's earlier
  failure.
- **`flui-interaction`**: a held key's release reaches the node that took its key-down after
  focus moved, so a keyboard scroll it began ends.
- **`flui-interaction`**: hover moves queued before a window leave, a pointer `Leave` or a
  device removal are dropped instead of being delivered after `on_exit`.
- **`flui-interaction`**: `GestureBinding::cancel_active_pointers` keeps the cancelled contacts'
  tails refused until a fresh Down, so a still-pressed pointer's moves no longer reach listeners
  as hover.
- **`flui-interaction`**: `MultiTapGestureRecognizer::builder` takes a validated
  `MultiTapCount`, now defined in `flui-interaction` and re-exported by `flui-widgets`, instead of
  asserting on a `usize`.
