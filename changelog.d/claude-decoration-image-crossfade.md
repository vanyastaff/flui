### Fixed

- **`flui-painting`**: `BoxDecoration::lerp` crossfades decoration images instead of switching
  at `t = 0.5`. The lerped decoration keeps `1 - t` of `a`'s images and `t` of `b`'s and paints
  them added together in a layer, so an opaque crossfade covers the box at every `t`; a lone
  image fades in or out over the background; the same image on both sides stays whole; and a
  lerp from a lerped decoration keeps the images it was blending.
