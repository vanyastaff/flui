### Changed

- **`flui`**: the facade turns no feature on by default
  ([ADR-0315](/docs/adr/ADR-0315-first-party-packages-remain-train-aligned-facade-features.md) §1).
  Migration: a plain `flui = { … }` no longer brings Material; add `features = ["material"]` to
  keep `flui::material` and opt into its Material-owned prelude. `default-features = false` is now
  redundant. In this repository, examples that use the catalog need `--features material`.
- **`flui create`**: the `basic` template is described as a stateless app; it never used a
  Material theme. Generated projects still name no catalog, since no template uses one.
