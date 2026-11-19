### Fixed

- **`flui-rendering`**: a merged semantics node in view is no longer published hidden because
  content merged into it lies past the paint clip (in a viewport's cache extent): the hidden
  flag the clips give a node now belongs to the node it forms, so it never crosses into the
  ancestor it merges into.
