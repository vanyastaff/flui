### Fixed

- Keep icon glyphs at their allocated logical size by default. Opting into
  `IconThemeData::apply_text_scaling` now scales the square and glyph together,
  including explicit icon sizes, without applying text scaling twice.
