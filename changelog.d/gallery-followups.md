### Added

- **`ScrollStep::contact`**: whether a user contact (a finger, a pen, a mouse with a button held)
  drives a nested scroll step, so a participant can tell a mouse drag from a mouse wheel, which
  share `PointerKind::Mouse` (ADR-0209 §2).

### Fixed

- **A wheel re-expands a `CollapsingHeader`**: a list at its extent still carries a wheel or
  trackpad packet through its scroll chain when a connection sits between it and the nearest
  enclosing scrollable, under the sequence's latch (ADR-0209 §5). Before, the list declined the
  packet and the header never saw it. `RefreshIndicator` pulls for a contact only, so a wheel at
  the top still pulls nothing.
- **A `Scrollable` without a controller keeps its position across rebuilds**: it no longer gets a
  fresh `ScrollController` each time its parent rebuilds, which cancelled a drag in progress
  (and a pull-to-refresh with it) and sent the offset back to zero.
- **A press focuses `RawTextField` and the Material `TextField`**: both requested focus from a
  tap, which the field's double-tap holds for the double-tap time, so the focus came back to the
  field half a second after a click and undid a Tab pressed meanwhile.
