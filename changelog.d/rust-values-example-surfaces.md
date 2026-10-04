### Fixed

- GPU examples create surfaces through wgpu's safe owned window and canvas APIs, removing the painting demo's leaked JavaScript value.
- The window example retains GPU state through the event loop and uses weak callback references to avoid a surface/window ownership cycle.
