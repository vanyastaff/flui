### Changed

- On iOS a ProMotion display runs at its highest rate only while a presentation animates or a pointer
  is down, and a still screen goes back to the system's adaptive rate. Before, the display link asked
  for no rate, so animations did not request the panel's maximum either.
