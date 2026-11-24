### Changed

- Unmounting a subtree of elements takes its render objects out of the render tree once, at the
  topmost one, as Flutter detaches only the topmost render object. Each element still finds its
  own render object present when it unmounts. A filter that drops 1500 of 3000 rows runs 2.4%
  fewer instructions in release, and so does scrolling a list one screen.
