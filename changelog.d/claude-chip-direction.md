### Fixed

- **`flui-material`**: `Chip` and `FilterChip` no longer document a left-to-right-only
  limitation they did not have: their content row follows the ambient `Directionality`, so in
  right-to-left the delete affordance and its tap target sit on the left. A test now pins it.
