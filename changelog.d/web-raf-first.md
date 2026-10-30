### Fixed

- Web: the `requestAnimationFrame` loop asks for its next frame before the current frame's work, so a frame that unwinds no longer stops the page from drawing.
