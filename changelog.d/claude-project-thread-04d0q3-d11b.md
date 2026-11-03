### Fixed

- **`flui-engine`**: a full frame under a window-sized antialiased clip evaluated all 64 samples
  of every leaf at every pixel, so a large window (a 6K display under one rounded corner, or
  fewer pixels under nested antialiased clips) went over the frame's clip work limit and every
  full frame was refused. Pixels well inside every leaf now take full coverage without
  sampling and are charged once, so the clip pays for its edges
  ([ADR-0102](/docs/adr/ADR-0102-immutable-geometric-clip-composition.md)).
