### Changed

- Color retains normalized floating-point sRGB and alpha through animation and paint. Components are private; use normalized accessors or explicit `to_rgba8()` byte export. Optional serde now uses normalized `srgb` and `alpha` fields instead of byte channels.
- Render opacity exposes accepted normalized values and retains fractional group opacity through composited layer updates.
- Color luminance uses linear sRGB; dark/light classification selects the greater black/white contrast. `luminance`, `is_dark`, `is_light` and `contrasting_text_color` are no longer const functions.

### Fixed

- Animated box and sliver opacity update child semantics when accepted opacity crosses zero, while honoring `always_include_semantics`.
- Image color filters preserve fractional alpha before paint modulation and filter source texels before interpolation. Neighboring filtered draws retain their own parameters.
- Antialiased isolated image draws apply independent clip coverage to source pixels. Filter preparation refusal clears rejected pending draws so later frames can recover.
- Color matrices avoid intermediate overflow for finite inputs; image filter preparation reports unsupported device binding limits before pipeline creation.
- Contrasting text selection avoids insufficient contrast on midgray, red and magenta backgrounds. Translucent surfaces must first be composited over their actual opaque backdrop.
