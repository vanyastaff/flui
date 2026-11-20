### Changed

- **`flui-material`**: an official package under `packages/flui-material`, built on `flui-sdk`
  alone ([ADR-0088](/docs/adr/ADR-0088-official-packages-sdk-and-facade.md) move 2). Its public
  API is unchanged; its manifest now depends on `flui-sdk` and `tracing` only.
- **`flui-macros`**: the derives look up `flui-sdk` in the consumer's manifest first, then the
  owning crate, then `flui`, so a package that depends on `flui-sdk` alone can use
  `StatelessView`, `StatefulView`, `InheritedData` and `Animatable`. The `Diagnosticable`
  derive has no path through `flui-sdk` yet.
