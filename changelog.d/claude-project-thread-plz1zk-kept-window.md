### Changed

- **`flui-platform`**: `SurfaceTarget` has a required `keep_past_close`. A window closed while its
  raster thread does not answer is hidden by its host instead of destroyed, so a wedged thread
  that still presents over it never reaches a destroyed native window. The window is closed to
  the application as before (ADR-0305 §9).
