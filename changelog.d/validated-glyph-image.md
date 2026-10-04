### Changed

- `GlyphImage` fields are private: construct images with `GlyphImage::try_new` and use read-only accessors. Invalid byte lengths and unrepresentable bitmap sizes return `GlyphImageError`; mutating a bitmap requires constructing a validated replacement. See [ADR-0120](/docs/adr/ADR-0120-validated-glyph-image.md) for migration and the retained rasterizer/atlas contracts.

### Fixed

- Glyph atlas admission rejects bitmap dimensions beyond the GPU texture limit or packer size range before evicting cached glyphs or growing pages; a later lookup can retry the rejected key.
