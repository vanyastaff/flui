### Added

- `examples/interaction_gallery`: one app with a screen per interaction widget (swipe actions, dismissible rows, pull to refresh, a collapsing header, a pager, a bottom sheet, zoom and pan, drag and drop, a context menu, a gesture pad, keyboard focus), from one widget tree on the desktop, Android and the web.
- `cargo xtask device windows-gallery`: drives every gallery screen on a real Windows window with mouse, keyboard and synthetic touch input, and checks each outcome through UI Automation and the window's pixels.

### Fixed

- A `LayoutBuilder` mounted in a list or a column no longer fails the layout of its ancestors for its first frame: before its builder runs it takes its smallest size, not an infinite one.
- An `InteractiveViewer` zoom scales the plane only; its depth scale made the compositor refuse every frame after the first zoom.
- Assistive technology follows keyboard focus onto a named `AnchoredDrag` (a bottom sheet's slider).
- A `GestureDetector` with both a drag slot and a transform slot pans with one finger or the mouse again; the two-contact scale no longer takes a lone contact's drag.
- A menu or dialog closed over a `WidgetsApp` home gives the keyboard focus back to the element that opened it.
