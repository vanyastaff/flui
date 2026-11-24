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

### Removed

- `RichText::on_link`: use `Text::rich(span).on_link(…)`.
