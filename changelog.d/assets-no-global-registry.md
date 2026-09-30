### Removed

- **`AssetRegistry::global()`** (`flui-assets`): the process-wide registry is gone
  ([ADR-0097](/docs/adr/ADR-0097-no-process-global-state-gate.md)). Build a registry with
  `AssetRegistryBuilder::new().with_default_capacity().build()` (the same 100 MB budget) and keep
  it where the application's other state lives.
