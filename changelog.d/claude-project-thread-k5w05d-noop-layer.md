### Fixed

- A layer that changes nothing where it composites (an image filter composing no filter) no
  longer isolates a backdrop filter under it in a partial frame alone: its children keep the
  damage scissor and draw straight into the parent, as in a full frame.
