### Fixed

- `Path` equality compares the shapes two paths name before their curves, so a rounded
  clip a scroll moved is found changed without building the corner arcs of either path.
  A scrolled text screen of Material cards spends a quarter fewer instructions per frame.
