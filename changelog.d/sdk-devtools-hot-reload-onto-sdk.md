### Added

- **`flui-sdk`**: `flui_sdk::hooks::FrameSnapshot`, an Evolving development hook that
  `flui-devtools`' timeline records
  ([ADR-0088](/docs/adr/ADR-0088-official-packages-sdk-and-facade.md) §4).

### Changed

- **`flui-devtools`**: an official package under `packages/flui-devtools`, built on `flui-sdk`
  alone ([ADR-0088](/docs/adr/ADR-0088-official-packages-sdk-and-facade.md) move 4). Its public
  API is unchanged; the `timeline` and `inspector` features no longer select individual
  framework crates, and the tree-observation seam test and overhead bench moved into it from
  `flui-testing` ([ADR-0040](/docs/adr/ADR-0040-tree-observation-seam.md) §8, amended).
