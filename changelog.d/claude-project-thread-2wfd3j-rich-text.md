### Added

- `Text::rich(TextSpan)` displays a span tree beneath the ambient `DefaultTextStyle`, as
  Flutter's `Text.rich` does, and `Text::on_link` activates its links.
- `RichText::text_sizing` and `RichText::font_weight_adjustment` size and weight a
  paragraph explicitly, as Flutter's `RichText` takes a `textScaler`.

### Changed

- `RichText` is the paragraph's render widget, as in Flutter: it reads the ambient
  `Directionality` while it configures its render object, and no longer reads the
  `MediaQuery` text sizing or weight adjustment, joins a `SelectionArea` or handles links.
  `Text` (and `Text::rich`) does all three, and builds one element fewer per run.
- A `RichText` given no `text_sizing` lays its spans out at their authored sizes, as
  Flutter's `RichText` defaults to `TextScaler.noScaling`; it no longer follows the
  platform's text scale. Use `Text::rich` for text that scales.

### Removed

- `RichText::on_link`: use `Text::rich(span).on_link(…)`.
