### Fixed

- **`flui-app`**: a frame that follows one that presented nothing waits for the pacing fallback,
  about one display period, even when the frame before it left build or layout work dirty. A tree
  that dirties itself every frame no longer runs frames at CPU speed into a window that shows none
  of them, such as a window whose raster thread died or an occluded window
  ([ADR-0353](/docs/adr/ADR-0353-production-is-paced-by-the-platform-and-a-frame-that-showed-nothing-waits-for-the-fallback.md)).
