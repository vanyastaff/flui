### Added
- `DragGestureRecognizer::handle_pan_zoom` reads a precision touchpad's two-finger pan as a drag, and a `GestureDetector` with a drag slot and no transform slot routes native pan/zoom to it. A canvas with only `on_pan_*` now pans from the touchpad on Windows and macOS.
- `PointerDispatch::with_root_to_local` names the transform that rewrote the event into the target's space.

### Fixed
- The ownership table measures an axial drag along its own axis: a horizontal drag inside a quarter-turn now wins a vertical motion of the root against an enclosing vertical scrollable, instead of losing it.
