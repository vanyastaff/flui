### Fixed

- **`flui-rendering`**: the hidden flag the clips give content past the paint clip (in a
  viewport's cache extent) now belongs to the node that content forms. A merged node in view is
  no longer published hidden because content merged into it lies past the clip, and an
  annotated child past the clip merges into its container as it does in view, instead of
  forming a hidden node of its own because two hidden flags conflicted.
