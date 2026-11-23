### Changed

- **`flui-painting`**: text is shaped with Parley 0.13. A line box is sized as CSS sizes it, the
  paragraph's root style included, so a rich paragraph whose root font differs from every span's
  can be a fraction of a pixel taller than before. A font without decoration metrics gets
  HarfBuzz's defaults in pixels from Parley (a line-through at half the ascent, where FLUI put it
  half an x-height above the baseline). Trailing whitespace hangs past the line end only where
  it collapses, as in CSS Text 4: preserved whitespace that does not wrap counts toward a line's
  width.
