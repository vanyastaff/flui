### Changed

- **The parity table has one source** (`flui-native`): `docs/platform-parity.md` is rendered from
  `crates/flui-native/platform-parity.toml`, and its summary counts, scores, readiness and the
  quality bar's tally are computed from the rows instead of kept by hand. Change a row in the TOML
  and restate the document with
  `FLUI_OVERWRITE=1 cargo test -p flui-native --lib parity_document_matches_its_source`; a hand
  edit of the document fails that test.
