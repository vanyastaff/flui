### Added

- `ToolbarItems` lays a toolbar's commands out in one row and puts the trailing ones that don't fit behind an overflow button, which shows them in a column. Hidden commands are not painted, hit or announced.
- `ToolbarItemsController` holds whether the toolbar is expanded and which items its row left out, so a design package can expand the toolbar itself or offer the hidden items in its own popup.
- `RenderToolbarItems` and `ToolbarItemsSplit` in `flui-objects` lay the row out and report the split.
- `WidgetsLocalizations::more_button_label` and `back_button_label` label the overflow button.

### Changed

- `PlainSelectionMenu`'s toolbar keeps to one row with a "⋯" button instead of wrapping onto more rows, so it no longer grows taller than the room over or under the selection.
