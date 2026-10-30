### Fixed

- **`flui-widgets`**: `Wrap` follows the ambient `Directionality`: in a right-to-left subtree each
  horizontal run starts at the right, and a vertical wrap places its first run on the right.
  `RenderWrap::set_text_direction` updates a laid-out wrap.
