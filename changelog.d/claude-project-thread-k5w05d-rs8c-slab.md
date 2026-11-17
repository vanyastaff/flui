### Fixed

- A repaint boundary removed after a frame's paint pass and before the frame's scene was reported (a lazy list evicting a child in the frame's tail) no longer leaves its record, or the scene nodes of the boundaries nested under it, for the rest of the owner's life.
