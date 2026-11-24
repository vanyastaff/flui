### Changed

- The Material Icons and Cupertino Icons faces are no longer in every application. Each is
  embedded under a feature of its own (`material-icons` and `cupertino-icons` of `flui`, `flui-sdk`
  and `flui-painting`), which `flui-material` and `flui-cupertino` turn on, so the `material` and
  `cupertino` catalogs bring their face with them. An application that draws one of the families
  with `Icon` and uses no catalog turns the facade's feature on; without it the icon draws the
  missing-glyph box. A generated counter, which uses no catalog, is 0.59 MB smaller on Linux
  x86_64 and 0.60 MB on wasm32 (ADR-0397).

### Fixed

- `BackButton` and the default delete icon of `Chip` drew the missing-glyph box, and `FilterChip`'s
  delete icon drew another glyph: their codepoints were not those of the embedded Material Icons
  face.
