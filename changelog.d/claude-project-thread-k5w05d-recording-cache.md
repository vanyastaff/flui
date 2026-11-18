### Changed

- The renderer keeps the draw recording of a chunk whose content, transform and surroundings are
  unchanged, or which only moved by whole device pixels (a list scrolled by whole pixels), and
  replays it instead of recording it again: a full frame of 60 text-heavy chunks takes about half
  the CPU it took. A chunk moved by a fraction of a pixel or under a fixed clip that cuts it, and
  chunks with a clip, a layer, a draw item or an external texture of their own, still record every
  time.
