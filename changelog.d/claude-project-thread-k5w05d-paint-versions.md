### Fixed

- **`flui-rendering`**: a repainted repaint boundary keeps the recording of every render object in
  it that owed no paint and did not move, so the engine replays those chunks instead of recording
  the whole boundary again. Before, every chunk of a repainted boundary took a new content
  version and missed the recording cache; a change to one cell of a grid without boundaries of
  its own now redraws only that cell
  ([ADR-0304](/docs/adr/ADR-0304-the-scene-is-retained-and-damage-is-the-footprint-of-its-patch.md)).
