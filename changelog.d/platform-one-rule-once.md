### Added

- `flui::platform::MAX_SURFACE_EXTENT`, the longest side in device pixels of a surface a window may ask a renderer to draw; the renderer and the native hosts read it instead of each stating the number.

### Changed

- `DisplayInfo::scale_factor` and `DisplayParts::scale_factor` are a `DevicePixelRatio`, finite and positive by its type, and `DisplayInfo::new` takes one: a snapshot can no longer hold a display whose scale is zero, negative or not a number. The headless platform's `DisplaySpec::scale_factor` and `DisplayChange::ScaleFactor` take one too.
- On Windows, a `SetSize` past the largest surface a renderer draws gives the window that surface instead of refusing the command with `PlatformFailure::OutOfRange`, as X11 already did; X11 now clamps a window extent at that surface rather than at its 16-bit range.
- A Wayland window draws the title bar of its own frame at the largest surface when the compositor sizes it past that, instead of drawing none.
