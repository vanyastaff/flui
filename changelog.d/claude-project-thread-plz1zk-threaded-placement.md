### Added

- A debug build on Linux or Windows turns the desktop loop's raster executor on its own raster
  thread when started with `FLUI_RASTER_PLACEMENT=threaded` (ADR-0305 §10): one frame presents
  while the next is built, and each window's wake settles what the thread finished. Release builds
  ignore the variable, and every other build stays inline; the switch is for the device runs that
  decide each platform's placement.
