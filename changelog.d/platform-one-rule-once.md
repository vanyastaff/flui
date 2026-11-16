### Added

- `flui::platform::MAX_SURFACE_EXTENT`, the upper bound in device pixels of a surface's side that a window may ask a renderer to draw. The renderer and the native hosts read it instead of each stating the number. The renderer still refuses a surface past its device's own limit, which can be lower.
- `flui::geometry::device_extent`, the one conversion of a logical surface side to whole device pixels (snapped, at least one pixel). The hosts and the runtime's scene surface use it, so the runtime no longer rounds a fractional side up where the host rounds it to the nearest pixel.
- `DevicePixelRatio::MIN` (1/32) and `DevicePixelRatio::MAX` (32), the range a device pixel ratio can be.

### Changed

- `DevicePixelRatio::try_new` refuses a ratio outside `MIN..=MAX`, not only zero, negative and non-finite ones, and is a `const fn`, so a constant ratio is checked at compile time. `WindowMetrics::try_new` and `from_logical` answer `InvalidMetrics::InvalidScale` for such a ratio.
- `PipelineOwner::set_device_pixel_ratio` takes a `DevicePixelRatio` instead of an `f64`. `UiRuntime::try_new` and `UiRuntime::with_capacity` refuse a ratio outside the range with the new `UiRuntimeError::PixelRatio`, and `UiRuntime::set_device_pixel_ratio_for` returns `false` for one, where a ratio such as 64 used to panic in the pipeline.
- `DisplayInfo::scale_factor` and `DisplayParts::scale_factor` are now a `DevicePixelRatio`, and `DisplayInfo::new` takes one, so a snapshot can no longer hold a display whose scale is out of that range. A recording that carries such a display scale is refused when it is read.
- A display or window scale that the operating system reports out of that range (zero, negative, not a number, or past any screen) now reads as one device pixel per logical pixel and is logged once per run of such reports from each source: each display, each window, the Windows system DPI and the X11 `Xft.dpi` keep their own run, and a display that goes away and returns starts afresh. A scale a window is told after it opens (`WM_DPICHANGED` on Windows, `preferred_scale` and `preferred_buffer_scale` on Wayland) is refused instead, and the window keeps its scale; a `WM_DPICHANGED` with a DPI such as 1 or 4000 no longer panics. Before, such a number could reach the display snapshot or a window's records, each host falling back its own way, and Android left the display out of the snapshot.
- On Windows, a `SetSize` past the largest surface now gives the window that surface instead of being refused with `PlatformFailure::OutOfRange`; a position past Win32's range is still refused. On Windows and X11 a size limit is held at the largest surface and rounded the same way on both, a minimum up and a maximum down, and X11 no longer sends a maximum below the minimum when both round across a pixel. X11 now clamps a window extent at the largest surface instead of at its 16-bit range.
- A Wayland window draws the title bar of its own frame at the largest surface when the compositor sizes it past that, instead of drawing none. Without a viewport, the bar's buttons are laid out within the part that is drawn. A compositor scale below the smallest ratio draws the buffer at one device pixel per logical pixel, and a content too long to fit the largest surface even at the smallest ratio publishes no metrics.

### Removed

- `InvalidMetrics::LogicalOverflow`: a logical size over a ratio in range is always finite.
