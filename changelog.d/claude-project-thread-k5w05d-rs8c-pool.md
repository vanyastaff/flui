### Added

- `SceneWriter::chunk` writes a repainted run into a chunk the scene owner reclaimed and hands back that chunk's emptied command list; `Canvas::record_into` records the next run into it. `DisplayList::clear` and `DisplayList::room` empty a list and report its capacity.

### Changed

- A repaint reuses the chunks and display lists a completed version released: at 1 000 leaves the repaint costs about one allocation per leaf instead of three.
