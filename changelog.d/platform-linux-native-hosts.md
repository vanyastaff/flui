### Fixed

- The native X11 host refuses a preference read once its loop stopped admitting work, as the
  Wayland host does, and a window opened during the bootstrap starts in the Settings portal's
  colour scheme instead of light.
