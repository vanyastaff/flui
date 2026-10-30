### Changed

- iOS and Web: a still screen no longer ticks. The `CADisplayLink` pauses, and the `requestAnimationFrame` loop stops, until a frame is asked for. A request from any thread resumes them. Android uses the same record of owed frames.
