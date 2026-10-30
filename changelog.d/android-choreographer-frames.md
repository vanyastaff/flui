### Changed

- Android: frames run on the choreographer's next frame, and a frame request carries its vsync (from API 24 on a 64-bit device, API 29 otherwise) and, from API 33, when the frame is expected on screen, so animations and idle work follow the display's timeline.
