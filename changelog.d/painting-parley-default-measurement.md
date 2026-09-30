### Changed

- `TextPainter` measures size, baselines and intrinsic widths with Parley on the realm's `TextContext` in every build (ADR-0092 §10 step 4a). Glyphs and carets still come from cosmic-text.
- Until host fonts reach the font collection, text in a family only the host has is measured in another face than it paints in. Named host families (Cupertino's `-apple-system`/`Segoe UI` chain among them) are measured in Roboto. CJK, emoji and other scripts the bundled faces do not cover are measured with no face while the host's fallback paints them, so they can overhang their box. A face registered through `shared_font_system().register_font` changes the painted text but not the measured size.
- The `flui-painting` features `parley` and `parley-layout` are removed. Migration: delete them from `features = [...]`; the Parley path is always compiled.
- With `bundled-fonts`, the process font system installs the bundled Roboto, in place of any host face named "Roboto", and binds its generic families to it. Text whose style names no family, names "Roboto" or names a generic (`monospace` included) is Roboto Regular on every host until host fonts reach the font collection: bold and medium weights paint as Regular, and `monospace` loses its fixed pitch. An app that relied on the host's sans-serif names the family.

### Removed

- `flui_painting::testing::measure_with_parley`: every painter measures on Parley.
