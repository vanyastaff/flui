### Added

- `PlatformInput::PointerFrame(PointerFrame)` carries the pointer events a backend sampled at one instant. The runtime delivers them as one gesture frame and records them as `pointer_frame` rows. The Win32 backend reports simultaneous touch and pen contacts this way.
