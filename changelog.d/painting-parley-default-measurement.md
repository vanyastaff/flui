### Changed

- `TextPainter` measures size, baselines and intrinsic widths with Parley on the realm's `TextContext` in every build (ADR-0092 §10 step 4a). Glyphs and carets still come from cosmic-text.
- The `flui-painting` features `parley` and `parley-layout` are removed. Migration: delete them from `features = [...]`; the Parley path is always compiled.
- With `bundled-fonts`, the process font system installs the bundled Roboto and binds its generic families to it, so text whose style names no family is Roboto on every host until host fonts reach the font collection. An app that relied on the host's sans-serif names the family.

### Removed

- `flui_painting::testing::measure_with_parley`: every painter measures on Parley.
