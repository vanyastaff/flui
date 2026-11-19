### Changed

- A chunk's kept draw recording no longer depends on the clip it is drawn under: the renderer
  applies the entering clip's scissor and rounded clip when it replays the recording, and leaves
  out the glyphs outside the scissor there. A list scrolled by whole device pixels under a fixed
  clip replays every row, the rows the clip's edges cut included, and a chunk drawn under two
  clips replays one recording under each.
