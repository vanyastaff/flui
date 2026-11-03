### Added

- `ReorderableList` and `ReorderableGrid` move their items: while a drag runs the dragged row
  leaves a gap where a drop would put it and the rows between make room, each sliding over
  250 ms; a drop or a cancel lands the row in its place from where it was let go, and a move
  command slides the row and its neighbours the same way. `proxy_decorator` dresses what the drag
  shows with a 150 ms lift (by default elevation 6 and 102 %). Under reduced motion every one of
  these lands on its next frame.
- `PlacementSlot::displace`: an animated list's owner paints one item away from where it is
  painted and slides it home on the run of its next roster change.
- `ReorderableGrid`: a lazy grid, laid out by any `SliverGridDelegate`, whose tiles the user moves
  by dragging a handle any way, or on touch and pen by a long press on the tile. A move is reported
  once as a `Reorder`, as `ReorderableList` reports it. Each tile's moves are "Move to the start",
  "Move to the end", the arrows along its row (one position) and across the rows (a whole row,
  onto the last tile when the next row is short). They follow the rows the grid lays out after a
  width change or in a right-to-left locale, and are assistive-technology actions, Alt with the
  arrows, Home and End, and a menu from the handle or visible controls. Each move is announced
  with the tile's new position.

### Changed

- A drag of a `ReorderableList` row or `ReorderableGrid` tile over the space after the last item
  keeps its gap, and a drop there lands in it; before, it was over no item and dropped nothing.
  The preview's `over` is the position a drop would move the item to, counted in the order shown.
- `assert_gesture_alternatives` walks a grid with roving focus row by row with the arrow keys onto
  the tile the assistive path invokes, instead of only down.
