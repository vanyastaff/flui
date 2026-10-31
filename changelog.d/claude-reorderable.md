### Added

- `ReorderableList`: a lazy list whose rows the user moves by dragging a handle, or on touch and
  pen by a long press on the row. A move is reported once as a `Reorder`, with `to` counted after
  the item leaves `from`. The same moves ("Move to the start", "Move up", "Move down", "Move to the
  end") are assistive-technology actions on the row, Alt with the arrows, Home and End while focus
  is in the row, and a context menu opened from the handle, or visible controls with
  `visible_controls`. Every move is announced with the row's new position
  (`WidgetsLocalizations::reorder_item_moved`), and a moved row keeps its focus.

### Changed

- `assert_gesture_alternatives` opens a row's menu through `ShowContextMenu` on the row that offers
  the command being checked, and reaches a row of a list with roving focus by its arrow keys when
  no Tab stop offers the command.
