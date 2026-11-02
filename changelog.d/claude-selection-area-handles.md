### Added

- `SelectionArea` selects by touch: a long press selects the word under it and shows a handle under each end of the selection; dragging a handle moves its end. A tap, Esc or a selection by mouse hides them. The handles live in the nearest `Overlay` and follow their ends through layer links, so they stay with the text as it scrolls.
- `SelectionHandles` builds the handles from a `HandleInfo` (`HandleSide`, text direction, line height). `DotHandles` draws round ones, the default; `SelectionArea::handles` sets them per area and `DefaultSelectionHandles` for a subtree.
- `RenderParagraph::set_selection_handles` publishes the start and the end of the paragraph's selection under layer links, each a leader at the bottom of its end's line.
