### Changed

- The renderer keeps the draw recording of a chunk whose content, transform and surroundings are
  unchanged and replays it instead of recording it again: a full frame of 60 text-heavy chunks
  takes about half the CPU it took. Chunks with a clip, a layer, a draw item or an external
  texture of their own still record every time.
