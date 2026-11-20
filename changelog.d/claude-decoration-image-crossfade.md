### Fixed

- **`flui-painting`**: `BoxDecoration::lerp` crossfades decoration images instead of switching
  at `t = 0.5`. The lerped decoration keeps `1 - t` of `a`'s images and `t` of `b`'s and paints
  them added together in a layer, so an opaque crossfade covers the box at every `t`. A lone
  image fades in or out over the background, the same image on both sides is one layer and does
  not dim, a lerp from a lerped decoration keeps the images it was blending, and past four
  layers the weakest go while the kept ones take up their shares.
