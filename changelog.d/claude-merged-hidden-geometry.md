### Fixed

- **`flui-rendering`**: the hidden flag the clips give content past the paint clip (in a
  viewport's cache extent) now belongs to the node that content forms. A merged node in view is
  no longer published hidden because content merged into it lies past the clip. A container and
  its annotated child both past the clip merge into one hidden node, as they merge in view,
  instead of each forming a hidden node because their two hidden flags conflicted.
