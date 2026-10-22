### Fixed

- **Web canvas backing store keeps to the pixel ratio** (`flui-native`): a `ResizeObserver`
  device-pixel size the current `devicePixelRatio` contradicts no longer becomes the backing
  store. Chrome's device emulation reports `devicePixelContentBoxSize` in the screen's own pixels
  while `devicePixelRatio` is the emulated ratio, so a 600×500 canvas at an emulated 2× kept a
  600×500 store, rendered blurry and reported 600×500 physical pixels at scale 2. A ratio change
  also no longer republishes the box reported at the old ratio until the observer reports the
  new one. The store is `round(CSS size × ratio)` until a report that fits arrives.
