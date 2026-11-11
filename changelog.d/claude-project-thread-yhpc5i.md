### Fixed

- On Windows, a touch or pen frame of several pointers reaches the app once, with each pointer's
  own position and state from the frame's history, and the lift that ends it is no longer lost:
  the platform stopped skipping the frame's remaining messages, which held back the next frame.
