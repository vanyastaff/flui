### Changed

- A root that is a repaint boundary (`RenderView`) now keeps its paint output
  between frames, so a dirty nested boundary no longer re-records the root, and
  a layer update directly under the root is patched instead of repainted.
  Changing the device pixel ratio drops all retained paint output.
