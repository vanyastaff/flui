### Changed

- **The workspace declares tiers, not layers**
  ([ADR-0081](/docs/adr/ADR-0081-workspace-tiers-and-reach-facts.md), which supersedes ADR-0041).
  `[package.metadata.flui]` no longer carries `layer = N`: every crate sets `tier`, `tier-kind`
  and a unique-within-tier `order`, examples and tools set only `tier-kind`, and the dependency
  rule is tier direction plus a smaller `order` inside one tier (`edge-exceptions` unchanged).
  `cargo xtask workspace` reports "N crates in M tiers, E dependency edges checked" and fails a
  manifest that still carries `layer` as an unknown key. The `reach` and `extends`/`forbid`
  tables are unchanged.
