### Changed

- **`flui-cupertino`**: an official package under `packages/flui-cupertino`, built on `flui-sdk`
  alone ([ADR-0088](/docs/adr/ADR-0088-official-packages-sdk-and-facade.md) move 3). Its public
  API is unchanged; its manifest now depends on `flui-sdk` and `tracing` only, and its
  `allowed-dependents` lists are replaced by the kind rule
  ([ADR-0081](/docs/adr/ADR-0081-workspace-tiers-and-reach-facts.md) §3).
