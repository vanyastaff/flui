### Changed

- `TextPainter` measures size, baselines and intrinsic widths with Parley on the realm's `TextContext` in every build (ADR-0092 §10 step 4a). Glyphs and carets still come from cosmic-text.
- Measurement runs over the app's font collection, which holds the host's faces, generics and fallback order, so named host families (Cupertino's `-apple-system`/`Segoe UI` chain among them), CJK and emoji measure in the faces they paint in. A context over `FontCollection::new()` (standalone pipelines, the hot-reload plugin) holds only the bundled faces and measures anything else in Roboto. A face registered through `shared_font_system().register_font` changes the painted text but not the measured size.
- The `flui-painting` features `parley` and `parley-layout` are removed. Migration: delete them from `features = [...]`; the Parley path is always compiled.
- With `bundled-fonts`, the process font system installs the bundled Roboto, Material Icons and CupertinoIcons, each in place of any host face of that family, and binds its generic families to Roboto. Text whose style names no family, names "Roboto" or names a generic (`monospace` included) is Roboto Regular on every host: bold and medium weights paint as Regular, and `monospace` loses its fixed pitch. An app that relied on the host's sans-serif names the family.
- A word wider than the line breaks between its glyphs in measurement as it does in paint, instead of overflowing the line.
- Letter spacing and line height set on a style without a font size apply at the default 14 px in paint as they do in measurement.
- `TextPainter::with_max_lines(Some(0))` / `set_max_lines(Some(0))` mean no limit and read back as `None`; `ParagraphSpec::max_lines` of `Some(0)` keeps every line.
- An empty paragraph's alphabetic baseline is the one a line of text in its style has (13.19 px at 14 px Roboto, was 8.40 px); its height is unchanged.
- Without `bundled-fonts`, the first family registered on an empty `FontCollection` becomes every generic family, so text that names no family measures in it.

### Added

- `flui_painting::testing::PROBE_MONO_100`, a generated test face (family "FLUI Probe Mono", `A` one em wide) for consumers' tests.

### Removed

- `flui_painting::testing::measure_with_parley`: every painter measures on Parley.
