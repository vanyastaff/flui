### Changed

- A paragraph the `TextContext` keeps is shared with its text and span records, and with the `ShapedParagraph` its first painter built: a later painter of the same paragraph paints those glyphs instead of collecting them from the layout again. A label toggling between two kept strings asks the heap for 10 fewer blocks per change.
