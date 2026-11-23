### Changed

- **`flui-painting`**: text is shaped with Parley 0.13. A line stays as tall as its runs make it,
  as in Flutter: the paragraph's root takes the font metrics of its first text, so a root style
  no span uses adds no height to Parley's CSS line box. A font without decoration metrics gets
  HarfBuzz's defaults in pixels from Parley (a line-through at half the ascent, where FLUI put it
  half an x-height above the baseline).
