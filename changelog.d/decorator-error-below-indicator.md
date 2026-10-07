### Fixed

- **`InputDecorator`** (`flui-material`): the helper or error line is laid out below the active
  indicator, outside the decorated container, 4dp below it and inset by the container's content
  padding so it starts where the content starts. It was drawn inside the field's box, above the
  underline. The container now keeps its content height instead of filling a tight parent, and a
  tap on the helper or error line no longer focuses a `TextField`.
