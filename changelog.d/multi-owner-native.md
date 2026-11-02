### Fixed

- Repaint a recreated GPU surface after recovery detected at the end of a frame, without waiting for another input.
- Release the application exit notifier before owner TLS teardown and close remaining Win32 windows before returning from the native loop.

### Added

- Testing-only native surface readback, recovery injection and a two-owner EditableText qualification example. Dedicated placement remains unavailable in production pending the complete native acceptance matrix.
