### Added

- `RenderUpdateImpact::SEMANTICS_GEOMETRY` in `flui-rendering`: a setter that only moves its
  subtree (a transform, a translation) asks the semantics pass to re-place the nodes inside it
  instead of re-assembling from the nearest semantics node above. `Transform`,
  `FractionalTranslation`, `RotatedBox`, `Container`'s transform and the animated transforms
  report it.

### Changed

- A slide or any other transform animation no longer re-assembles the whole semantics tree on
  each frame while semantics are on; a steady slide frame allocates 4 times instead of 21.
