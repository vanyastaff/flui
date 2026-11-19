### Fixed

- A partial frame replays only the chunks of a repaint boundary that its damage can reach, instead
  of the boundary's whole recording: on a 60 × 60 grid painted into one boundary, a one-cell
  update's raster CPU drops from 4.0 to 1.1 ms on lavapipe.
