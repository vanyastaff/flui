### Fixed

- **`flui-widgets`**: `Text` and `RichText` without an explicit `direction` read the ambient
  `Directionality` instead of defaulting to left-to-right, so in RTL their paragraph is
  right-to-left and start-aligned text sits at the right edge. An explicit `direction` still
  wins.
