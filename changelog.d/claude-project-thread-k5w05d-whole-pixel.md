### Changed

- A chunk moved by whole device pixels, such as a list scrolled by whole pixels, replays its kept draw recording at the new position instead of recording again. A chunk moved by a fraction of a pixel, or moved under a fixed clip that cuts it, still records again.
