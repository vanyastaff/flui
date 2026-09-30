### Added

- `flui_painting::HostFonts`: one scan of the host's installed fonts (`HostFonts::scan`, fontdb),
  with the generic families and per-platform fallback lists FLUI picks for it; a value, with no
  process-global state ([ADR-0092](/docs/adr/ADR-0092-per-realm-text-over-parley.md) §10
  step 6a).
- `FontCollection::with_host_fonts(&HostFonts)`: a collection fed from a host scan. With
  `bundled-fonts` the bundled Roboto keeps every generic family and a host copy of a bundled
  family is never added.
- `flui_painting::testing::host_fed(&FontCollection)`: whether a collection was fed from a host
  scan.

### Changed

- The `flui_painting::testing` helpers `host_covers`, `host_chain_covers`,
  `host_sans_serif_family` and `host_family_names` take the `&HostFonts` they answer for.
- The app scans the host's fonts itself and feeds its collection from that scan; the fallback
  lists are FLUI's own, per platform, and match what cosmic-text 0.19 used.

### Removed

- cosmic-text, and with it `flui_painting::SharedFontSystem`, `flui_painting::shared_font_system`,
  `flui_painting::text_layout::{init_font_system_with_faces, font_system_initialized}`,
  `flui_painting::testing::host_face_feeds` and `FontCollection::with_host_faces`. Migrate
  `FontCollection::with_host_faces(&shared_font_system())` to
  `FontCollection::with_host_fonts(&HostFonts::scan())`.
- `flui_testing::fonts` and `flui_testing::pin_font_faces`: test realms shape on a bundled-only
  collection, so there is no font state to pin; delete the calls.
