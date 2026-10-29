### Fixed

- **Web**: a page's `flui-canvas` laid out at its own size (no CSS width or height) keeps its box.
  Such a side is as large as the backing store written into the canvas's `width`/`height`, so
  every resize report grew it without bound at a pixel ratio above 1 or with padding, and shrank
  it below 1. Opening a window now pins each such side at the box it has, a CSS minimum it sits
  on included, and leaves the sides the page sizes, a CSS width with an `auto` height included, to
  the page.
