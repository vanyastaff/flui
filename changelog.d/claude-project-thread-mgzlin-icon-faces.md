### Changed

- The Material Icons and Cupertino Icons faces are no longer in every application: `bundled-fonts`
  embeds Roboto alone, and each icon face has a feature of its own (`material-icons` and
  `cupertino-icons` of `flui`, `flui-sdk` and `flui-painting`, off by default). `flui-material`
  turns on `material-icons`, so the `material` catalog brings the face its widgets draw with. The
  `cupertino` catalog draws no glyph of the Cupertino family and does not bring its face. An
  application that draws a family with `Icon` and has no catalog that brings it turns the facade's
  feature on; without it the icon draws the missing-glyph box on every host. A generated counter,
  which uses no catalog, is about 0.6 MB smaller on Linux x86_64 and on wasm32 (ADR-0397).
- A host font feed never adds a face of the "Material Icons" or "CupertinoIcons" family: an icon
  draws from the face FLUI embeds or from one the application registers, never from a copy the
  host happens to have.
- `flui_painting::fonts::MATERIAL_ICONS_REGULAR` and `CUPERTINO_ICONS` exist only under
  `material-icons` and `cupertino-icons`. Migration: turn the feature on where code names the
  constant.

### Fixed

- `BackButton` and the default delete icon of `Chip` drew the missing-glyph box, and `FilterChip`'s
  delete icon drew another glyph: their codepoints were not those of the embedded Material Icons
  face.
