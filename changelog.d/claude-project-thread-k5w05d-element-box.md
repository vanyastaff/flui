### Changed

- **`flui-widgets`, `flui-view`**: a control on the interactive node mounts with fewer heap
  bytes. The node holds its action chain and its recognizer factories once instead of beside
  the copies its tap slot and chain link already hold, a focus configuration keeps its external
  node beside one flag, a focus host's two focus flags share a word, and every element's core
  holds its depth in 32 bits. The counts are the baselines
  ([`raw_button_over_text`](/crates/flui-widgets/perf/baseline/raw_button_over_text.toml), the
  scenarios in `/packages/flui-material/perf/baseline/` and
  `/packages/flui-cupertino/perf/baseline/`).
