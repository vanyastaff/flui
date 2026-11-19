### Fixed

- **`flui-painting`**: `BoxDecoration::lerp` crossfades decoration images instead of switching
  at `t = 0.5`: the lerped decoration paints `a`'s image at `1 - t` times its opacity and `b`'s
  over it at `t` times its own, and a lone image fades in or out. An animated decoration that
  changes its image no longer pops at the midpoint.
