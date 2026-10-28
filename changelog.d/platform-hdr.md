### Added

- Each display in the snapshot carries its colour profile, `DisplayInfo::color`: the
  `SurfaceColorSpace` surfaces are shown in (a `ColorGamut` and a `DynamicRange`) and its
  `Headroom` over SDR white, or the `CapabilityAbsence` that says why the platform gives none.
  `DisplayInfo::color_space()` and `DisplayInfo::headroom()` answer that absence as `Unsupported`.
  A headroom is finite and at least 1 by construction (`Headroom::new`,
  `Headroom::from_luminance`). HDR switching on or off, or the headroom moving, is a new snapshot
  revision. Windows reads `IDXGIOutput6::GetDesc1` and the SDR white level; macOS
  `canRepresentDisplayGamut:` and the potential extended-dynamic-range headroom; iOS the display
  gamut and `potentialEDRHeadroom`; Android `isWideColorGamut`, `isHdr` and
  `getHighestHdrSdrRatio`; Wayland `wp_color_management_v1` where the compositor has it; the Web
  the `dynamic-range` and `color-gamut` media queries. X11 answers `NotYetOnThisHost`.
- The headless platform injects a profile: `DisplaySpec::with_color` and
  `DisplayChange::Color`.

### Changed

- `Renderer::with_surface_color` takes the `DisplayColor` of the window's display (`None` where
  the platform reports none). Where there is one, it decides whether an `Hdr` request can be met
  and bounds the tone mapping by its headroom, ahead of what the surface reports;
  `ResolvedSurfaceColor::resolve` takes it too. The app hands each renderer the colour of the
  display its window is on.
