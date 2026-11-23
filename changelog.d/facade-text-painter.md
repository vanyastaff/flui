### Added

- `flui::painting` exports `TextPainter`, `TextMeasurement`, `TextMeasurementError`,
  `TextPreparationPending`, `TextLayoutError`, `TextResolvedSize`, `Invalidation` and
  `ShapedParagraph`, so a render object of an application's own shapes text in layout through
  `ctx.text()` and paints it without depending on `flui-painting`.
