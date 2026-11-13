### Changed

- A `Path` made as a rectangle, oval or rounded rectangle, or an empty `Path`, builds its curves only when they are read, so a rounded surface that is recorded, clipped, moved and shadowed allocates no path geometry. An `ElevatedButton` leaves 1 808 bytes and 6 allocations less per button.
