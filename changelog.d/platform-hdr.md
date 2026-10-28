### Added

- Each display in the snapshot carries its colour profile, `DisplayInfo::color`: the
  `SurfaceColorSpace` surfaces are shown in (a `ColorGamut` and a `DynamicRange`) and its
  `Headroom` over SDR white, or the `CapabilityAbsence` that says why the platform gives none.
  `DisplayInfo::color_space()` and `DisplayInfo::headroom()` answer that absence as `Unsupported`.
  A headroom is finite and at least 1 by construction (`Headroom::try_new`,
  `Headroom::from_luminance`). HDR switching on or off, or the headroom moving, is a new snapshot
  revision. Windows reads `IDXGIOutput6::GetDesc1` and the SDR white level; macOS
  `canRepresentDisplayGamut:` and the potential extended-dynamic-range headroom; iOS the display
  gamut and `potentialEDRHeadroom`; Android `isWideColorGamut`, `isHdr` and
  `getHighestHdrSdrRatio`; Wayland `wp_color_management_v1` where the compositor has it; the Web
  the `dynamic-range` and `color-gamut` media queries. X11 answers `NotYetOnThisHost`.
- The headless platform injects a profile: `DisplaySpec::with_color` and
  `DisplayChange::Color`.
- `WindowDisplay::color` (and `WindowDisplay::with_color`): a window learns that its display's
  colour changed, and `Renderer::set_display_color` configures its surface again for it before
  the next frame; the desktop, Android, iOS and Web runners call it.
- `OsWindowOptions::android` (`AndroidWindowOptions`, `AndroidColorMode`): the Activity's
  window asks for a wide-gamut or HDR colour mode (`Window.setColorMode`, API 26 and later). The
  app sets it from `AppConfig::surface_color` where the application set no Android options.

### Changed

- `Renderer::with_surface_color` takes the `DisplayColor` of the window's display (`None` where
  the platform reports none). Where there is one, it decides whether an `Hdr` request can be met
  (a headroom no more than SDR white is SDR) and bounds the tone mapping by its headroom, ahead
  of what the surface reports, and a display reported in sRGB gets no `WideGamut` surface;
  `ResolvedSurfaceColor::resolve` takes it too. The app hands each renderer the colour of the
  display its window is on.
