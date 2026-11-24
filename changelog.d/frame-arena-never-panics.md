### Fixed

- `flui_foundation::arena::FrameArena` has no panic path left in the code that hands out memory:
  a list of chunks that cannot grow is now an `AllocError`, which a collection's `try_` methods
  return, as they already did for a chunk the global allocator refused.
