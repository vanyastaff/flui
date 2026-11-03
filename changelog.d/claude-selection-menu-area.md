### Added
- `SelectionArea` has a selection menu. A secondary click, the menu key or assistive technology's `ShowContextMenu` on its document opens it as a popup; a click off the selection selects the word there on Apple platforms and clears the selection elsewhere. A touch long press or letting go of a dragged handle shows it as a toolbar over the selection. Both offer copy while something is selected and select all, then the commands of `DefaultSelectionControls` and the area. `SelectionArea::menu`, `no_menu`, `menu_policy` and `menu_command` set it per area.

### Changed
- A `SelectionArea` always answers `DismissIntent`: with nothing selected it hands Esc to the enclosing answer, as before, so its actions no longer change with the selection.
