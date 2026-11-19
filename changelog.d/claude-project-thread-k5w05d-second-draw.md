### Changed

- **`flui-engine`**: the recording cache captures a chunk's recording only on the second frame
  that draws it under one key, as Flutter's raster cache waits for a picture to repeat, so a
  first frame and a frame of content that changes every frame no longer pay for copying
  recordings they never replay; on the `raster_share` bench under lavapipe a first frame of a
  60 × 60 grid spent 13 to 16 ms of raster CPU against 37 ms before
  ([ADR-0304 §9](/docs/adr/ADR-0304-the-scene-is-retained-and-damage-is-the-footprint-of-its-patch.md)).
