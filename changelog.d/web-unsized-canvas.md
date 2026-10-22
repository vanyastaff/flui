### Fixed

- **Web**: a page's `flui-canvas` with no CSS size no longer grows without bound on a display with
  a pixel ratio above 1. Such a canvas is laid out at the backing store written into its
  `width`/`height`, so every resize report grew it; opening a window now pins each side the store
  sizes at the box it has, and leaves the sides the page sizes, a CSS width with an `auto` height
  included, to the page.
