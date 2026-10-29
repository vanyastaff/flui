### Added

- `GestureBinding::dispatch_pointer_frame_on` dispatches the contacts a platform reported at one instant as one frame, and a flush of coalesced moves is one too. A recognizer that measures several contacts joins the open frame (`GestureArena::join_pointer_frame`) and settles in `GestureRecognizer::pointer_frame_end`, once per frame.

### Fixed

- A pinch (`ScaleGestureRecognizer`) is measured once per frame with every finger where it moved to, not once per finger with the others still in place, so its scale, rotation and focal point no longer jump between the intermediate states.
