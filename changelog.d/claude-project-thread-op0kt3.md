### Added

- `ReorderableGrid`: a lazy grid, laid out by any `SliverGridDelegate`, whose tiles the user moves
  by dragging a handle any way, or on touch and pen by a long press on the tile. A move is reported
  once as a `Reorder`, as `ReorderableList` reports it. Each tile's moves are "Move to the start",
  "Move to the end", the arrows along its row (one position) and across the rows (a whole row,
  onto the last tile when the next row is short). They follow the rows the grid lays out after a
  width change or in a right-to-left locale, and are assistive-technology actions, Alt with the
  arrows, Home and End, and a menu from the handle or visible controls. Each move is announced
  with the tile's new position.

### Changed

- `assert_gesture_alternatives` walks a grid with roving focus row by row with the arrow keys onto
  the tile the assistive path invokes, instead of only down.
