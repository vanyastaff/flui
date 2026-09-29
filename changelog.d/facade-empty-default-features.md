### Changed

- **`flui`**: the facade turns no feature on by default
  ([ADR-0088](/docs/adr/ADR-0088-official-packages-sdk-and-facade.md) §6). Migration: a plain
  `flui = { … }` no longer brings Material; add `features = ["material"]` to keep
  `flui::material` and the Material names in `flui::prelude` (`Scaffold`, `ElevatedButton`,
  `Theme`, `TextField`, …). `default-features = false` is now redundant. In this repository,
  `cargo run --example material_demo` needs `--features material`.
- **`flui create`**: the `basic` template is described as a stateless app; it never used a
  Material theme. Generated projects still name no catalog, since no template uses one.
