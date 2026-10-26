### Fixed

- A `LayoutBuilder` mounted in a list or a column no longer fails the layout of its ancestors for its first frame: before its builder runs it takes its smallest size, not an infinite one.
- An `InteractiveViewer` zoom scales the plane only; its depth scale made the compositor refuse every frame after the first zoom.
- Assistive technology follows keyboard focus onto a named `AnchoredDrag` (a bottom sheet's slider).
- A `GestureDetector` with both a drag slot and a transform slot pans with one finger or the mouse again; the two-contact scale no longer takes a lone contact's drag.
- A menu or dialog closed over a `WidgetsApp` home gives the keyboard focus back to the element that opened it.
