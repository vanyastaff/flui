### Changed

- **`flui-runtime`**: a presentation leaving `Detached` climbs `Paused, Hidden, Inactive,
  Resumed` up to its state, so a window that starts detached and comes up unfocused or in the
  background never reports `Resumed` on the way
  ([ADR-0248](/docs/adr/ADR-0248-leaving-detached-enters-from-paused.md)).

### Fixed

- **`flui-app`**: a window command that panics no longer stops the other windows' queued
  commands, and the window it interrupted asks for the turn that applies the rest. On Android
  and iOS the turn a command's wake brings now applies it, instead of leaving it until
  unrelated input.
- **`flui-app`**: commands queued after a window close that goes ahead are dropped, not applied
  to the destroyed window; after a vetoed close they still apply.
- **`flui-app`**: a surface resize that arrives while the window's frame driver is out on a frame
  reaches the driver at its next checkout instead of being lost.
- **`flui-runtime`**: the shared frame budget ignores hidden and closing windows, so a minimised
  window on a faster display no longer makes every frame of a visible window count as janky.
