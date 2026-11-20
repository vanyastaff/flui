### Changed

- **Inherited lookups copy fewer tables** (`flui-view`): where an element's inherited scope
  keeps its folded tables now depends only on provider depth. The provider at each multiple of
  four builds one table, and every provider four to seven levels below it shares that table. A
  control whose own providers nest fewer than four deep copies none, however many providers sit
  above it. A lookup scans at most seven layers and then one table. Chips, buttons, tab bars and
  navigation bars mount with fewer bytes and allocations; the counts are in the Material and
  Cupertino `perf/baseline/` files, such as
  [`chip_over_text.toml`](/packages/flui-material/perf/baseline/chip_over_text.toml). The
  per-row `cupertino_button_over_text` figure rounds up by one byte, although both of its
  1000-row columns fall: the button rows from 19,905,514 to 19,905,482 live bytes and the
  bare-`Text` rows from 7,987,854 to 7,987,374.
