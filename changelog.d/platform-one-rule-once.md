### Added

- `flui::platform::MAX_SURFACE_EXTENT`, the upper bound in device pixels of a surface's side that a window may ask a renderer to draw. The renderer and the native hosts read it instead of each stating the number. The renderer still refuses a surface past its device's own limit, which can be lower.

### Changed

- `DisplayInfo::scale_factor` and `DisplayParts::scale_factor` are now a `DevicePixelRatio`, finite and positive by its type, and `DisplayInfo::new` takes one, so a snapshot can no longer hold a display whose scale is zero, negative or not a number. A recording that carries such a display scale is refused when it is read.
- A display or window scale that the operating system reports as zero, negative or not a number now reads as one device pixel per logical pixel and is logged once per run of such reports. Before, such a number could reach the display snapshot or a window's records, each host falling back its own way, and Android left the display out of the snapshot.
- On Windows, a `SetSize` past the largest surface now gives the window that surface instead of being refused with `PlatformFailure::OutOfRange`; a position past Win32's range is still refused. On Windows and X11 a size limit past the largest surface is held at it, and X11 rounds a fractional limit as Windows does: a minimum up, a maximum down. X11 now clamps a window extent at the largest surface instead of at its 16-bit range.
- A Wayland window draws the title bar of its own frame at the largest surface when the compositor sizes it past that, instead of drawing none. Without a viewport, the bar's buttons are laid out within the part that is drawn.
