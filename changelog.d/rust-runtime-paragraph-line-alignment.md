### Fixed

- Center, right and directional text alignment position each line inside the actual allocated paragraph box; loose wrap caps no longer shift text beyond its reported width.
- Justified paragraphs stretch eligible soft lines using the same positions for glyphs, carets and selection. Changing alignment refreshes cached positions.

### Changed

- `ParagraphSpec` accepts `min_width` and `text_align` separately from its wrap cap.
