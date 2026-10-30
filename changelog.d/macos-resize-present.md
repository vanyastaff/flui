### Fixed

- macOS: a live resize presents each frame in the same Core Animation transaction as the new window frame, so the dragged edge no longer shows a stretched old frame or a strip of background.
- macOS: turning the title bar or resizing on or off no longer draws a frame inside the window command; the frame is asked for and comes on the next display pass.
