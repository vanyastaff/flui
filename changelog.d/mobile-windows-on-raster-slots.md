### Changed

- Android and iOS windows draw through the application's one GPU context, as desktop windows do: every window is a slot of the inline raster executor, so a second window no longer builds its own device, pipelines and glyph atlas, and ADR-0267's device-rebuild backoff is held once, by the executor.
- On Android and iOS a window whose GPU device was given up after repeated rebuild failures is tried again when the platform gives its surface back (the user leaves the application and returns).

### Fixed

- A window surface the GPU backend refuses to make on Android or iOS is asked for again at a paced interval (16 ms doubling to 1 s) instead of on every frame.
