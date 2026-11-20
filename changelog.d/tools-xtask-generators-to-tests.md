### Removed

- `tools/xtask` and the `cargo xtask` alias. The generators are tests of the crates that own their
  output, and `FLUI_BLESS=1` rewrites what they generate: `flui-platform`'s `key_vocabulary`,
  `flui-testing`'s `recording_vocabulary_table`, `flui-material`'s
  `material_icon_constants_match_their_frames`, `flui-cli`'s `platform_floor`, and the
  `font_assets` and `android_dex` checks. The Android dex is built by `android-dex.yml` with
  `mode: write`, and the changelog check is a step of CI's `checks` job. The SVG and AVD icon
  importers are dropped.
