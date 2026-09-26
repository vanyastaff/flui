### Added

- **`cargo xtask workspace`**: the kind rule of
  [ADR-0081](/docs/adr/ADR-0081-workspace-tiers-and-reach-facts.md) §3. A crate that is neither
  an official package nor an application names an official package in no dependency kind (dev
  and optional included), and an official package's normal and build dependencies are
  `flui-sdk`, `flui-platform-api` and `flui-protocol`; an edge between official packages is
  refused in every dependency kind, dev included; each refused edge needs the dependent's
  `edge-exceptions` entry. A member under `packages/` must be official and list no exception.

### Changed

- **`flui-material`**: an official package under `packages/flui-material`, built on `flui-sdk`
  alone ([ADR-0088](/docs/adr/ADR-0088-official-packages-sdk-and-facade.md) move 2). Its public
  API is unchanged; its manifest now depends on `flui-sdk` and `tracing` only.
- **`flui-macros`**: the derives look up `flui-sdk` in the consumer's manifest first, then the
  owning crate, then `flui`, so a package that depends on `flui-sdk` alone can use
  `StatelessView`, `StatefulView`, `InheritedData` and `Animatable`. The `Diagnosticable`
  derive has no path through `flui-sdk` yet.
