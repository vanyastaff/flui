### Added

- **`CancelReason::Superseded`** (`flui-platform-api`): the framework ends a contact with it when a
  newer contact takes its place. `PointerMove::COALESCED_HISTORY_LIMIT` names the measured history a
  queue that folds moves keeps.

### Changed

- **The contact limit cancels the oldest contact** (`flui-interaction`): a Down at the binding's
  32-contact limit cancels the oldest contact, once, with `Cancel(Superseded)`, instead of being
  refused, so a backend that loses Ups (and never reuses pointer ids) can no longer lock input out.
- **`HitTestResult::new` allocates nothing** (`flui-interaction`) and is `const`; an empty transform
  stack is the identity.

### Fixed

- **A pointer's `Leave` ended nothing** (`flui-interaction`): a pen lifted out of range, or any
  source's own `Leave`, now exits the regions it hovered, gives the cursor back and forgets the
  source, so the next frame's hover refresh no longer re-enters them. Removing a device does the
  same; it used to drop the hover without a single `on_exit`.
- **Hover kept departed regions alive** (`flui-interaction`): regions no source hovers any more
  after a window leave, a pointer leave or a device removal release their callbacks.
- **Embedded surfaces shared hover regions by render id** (`flui-interaction`): two surfaces whose
  trees reuse an id kept one region between them, so one surface's exit ran the other's callback.
- **A superseding Down left the old contact hanging** (`flui-interaction`): a Down for a pointer
  whose earlier contact never ended now delivers that contact's owed motion and one
  `Cancel(Superseded)` before the new Down, instead of abandoning its route silently.
- **A refused contact blocked hover** (`flui-interaction`): a mouse or pen whose contact was
  refused (cancelled on focus loss while pressed, or past the refusal table) hovers again on its
  first move with every button up; a fresh Down always ends a refusal.
- **A pressed move with no contact reached targets as a drag** (`flui-interaction`): a drag begun
  outside the window, or a Down that was never admitted, now reaches the pointer router only,
  while hover follows a mouse or pen.
- **An unregistered target kept hearing its contact's moves** (`flui-interaction`): a target
  unregistered mid-contact now hears only that contact's Up or Cancel.
- **Hover stayed stale after a drag or scroll latch ended** (`flui-interaction`): the binding asks
  its window for a frame when hover unfreezes, so the ambient refresh runs without another move.
- **Drop receivers under a perspective transform got a wrong local point** (`flui-interaction`):
  receivers are localized onto their own plane as pointer targets are, and one the drag cannot
  reach is skipped.
- **Coalesced moves copied and grew their history** (`flui-interaction`, `flui-runtime`): folding a
  frame's moves, in the binding and in the runtime's held input, moves the older history instead
  of cloning it and keeps at most `PointerMove::COALESCED_HISTORY_LIMIT` readings.
- **Hover moves cloned their hit path and allocated per move** (`flui-interaction`): the path is
  resolved once and handed to the frame queue, and an unchanged hover keeps its stored regions
  without allocating.
