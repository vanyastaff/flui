### Changed

- `InkWell`, `Card`, `Dialog` and `FloatingActionButton` take their child with `.child(…)` instead of an argument of `new`, as Flutter leaves each child nullable; without one they lay out, paint and take hits as an empty surface.
