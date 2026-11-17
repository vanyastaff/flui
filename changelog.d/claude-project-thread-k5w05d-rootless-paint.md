### Fixed

- A frame that runs after the render root was removed no longer reports `painted = true` for a patch staged by an earlier failed frame, and that patch is dropped instead of being committed by a later report.
