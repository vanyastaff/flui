### Changed

- iOS: a frame request carries the display link's vsync and target present time, not the time the main thread handled the tick, so animations and pointer resampling run on the display's timeline.
