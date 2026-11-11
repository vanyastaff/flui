### Changed

- `ShapedParagraph::describe_spans` returns `Vec<String>`, formatted when called: a shaping records the spans' values and no longer formats their snapshot lines each time it shapes.
- Turning a layout into the paragraph the display list carries sizes its run, glyph and line lists from the layout and finds faces by a scan, instead of growing each list and hashing faces in a `std` map.
- A `TextPainter` whose text changes shapes the new text into the buffers of the layout it dropped, instead of allocating a fresh parley layout each time.
