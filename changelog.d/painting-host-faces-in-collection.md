### Added

- **`FontCollection::with_host_faces`** (`flui-painting`): a font collection fed from the process
  font system: the host faces it discovered, its generic families and its fallback order
  ([ADR-0092](/docs/adr/ADR-0092-per-realm-text-over-parley.md) §7).

### Changed

- The app's font collection (`flui-app`) is built with `FontCollection::with_host_faces` once per
  app, before the first frame, so Parley measures CJK, emoji and host-named families (Cupertino's
  `-apple-system`, `system-ui`, `Segoe UI` chain) in the faces cosmic-text paints them with. The
  Parley path hands the shaper the one family FLUI's family rule resolves, and falls back past
  it in the process font system's order, ending at the sans-serif family. A family name matches
  only as the fonts spell it, on both sides, and a bundled-only collection
  (`FontCollection::new()`) falls back to Roboto for every script.
