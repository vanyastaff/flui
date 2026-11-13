### Changed

- A `Path` made as a rounded rectangle, or an empty `Path`, builds its curves only when they are read, and a translate of such a path records the move, so a rounded surface that is recorded, clipped, moved and shadowed allocates no path geometry. An `ElevatedButton` leaves 1 792 bytes, 6 allocations and 2 reallocations less per button.
