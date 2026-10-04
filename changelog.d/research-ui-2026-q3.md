### Fixed

- Update the AccessKit adapter cohort to include Unix enabled/sensitive states and the AT-SPI Image interface.
- Retry glyphs whose bitmap cannot be reproduced when the atlas grows instead of retaining an empty cached slot; protect slots referenced by the current frame until retirement.
- Reject color glyph bitmaps whose byte count overflows instead of panicking during validation.

### Changed

- Avoid collecting matches and allocating the label query on successful unique accessibility queries while preserving complete ambiguity diagnostics.
- Recommend nextest 0.9.145 or newer to avoid false capture-pipe leak reports; the required version remains 0.9.133.
