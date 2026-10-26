### Added
- `AppConfig::with_surface_color(SurfaceColor)` and `Renderer::with_surface_color` request a wide-gamut (`WideGamut`) or extended-range (`Hdr`) window surface. The request falls back `Hdr` → `WideGamut` → `Srgb8` when the surface, adapter or display cannot meet it, and `Renderer::surface_color()` reports what was granted. Painting is unchanged; a granted non-`Srgb8` surface is converted from an `Rgba16Float` working target at presentation, with hue-preserving tone mapping bounded by the display headroom.

### Changed
- `Renderer::select_surface_format` is gone; the 8-bit sRGB pair is chosen by `ResolvedSurfaceColor::resolve`.
