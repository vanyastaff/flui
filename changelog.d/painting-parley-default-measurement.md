### Changed

- `TextPainter` measures size, baselines and intrinsic widths with Parley on the realm's `TextContext` in every build (ADR-0092 §10 step 4a). Glyphs and carets still come from cosmic-text.
- Measurement runs over the app's font collection, which holds the host's faces, generics and fallback order, so named host families (Cupertino's `-apple-system`/`Segoe UI` chain among them), CJK and emoji measure in the faces they paint in. A context over `FontCollection::new()` (standalone pipelines, the hot-reload plugin) holds only the bundled faces and measures anything else in Roboto. A face registered through `shared_font_system().register_font` changes the painted text but not the measured size.
- The `flui-painting` features `parley` and `parley-layout` are removed. Migration: delete them from `features = [...]`; the Parley path is always compiled.
- With `bundled-fonts`, the process font system installs the bundled Roboto, in place of any host face named "Roboto", and binds its generic families to it. Text whose style names no family, names "Roboto" or names a generic (`monospace` included) is Roboto Regular on every host: bold and medium weights paint as Regular, and `monospace` loses its fixed pitch. An app that relied on the host's sans-serif names the family.

### Removed

- `flui_painting::testing::measure_with_parley`: every painter measures on Parley.
