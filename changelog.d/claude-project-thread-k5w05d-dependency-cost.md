### Changed

- **`DependentEntry::depth` is a `u32`** (`flui-view`): it was a `usize`. The ledger converts the
  build context's depth once, where it records the entry, and treats a tree deeper than
  `u32::MAX` levels as a bug instead of truncating it.
- **Inherited reads cost less memory** (`flui-view`): a provider read by more than eight
  dependents indexes them by position alone, and a dependent that reads more than two providers
  sizes its provider list once per build instead of doubling it. Every control over the
  ADR-0322 node mounts with fewer bytes and allocations; the counts are in
  [`raw_button_over_text.toml`](/crates/flui-widgets/perf/baseline/raw_button_over_text.toml)
  and the Material and Cupertino `perf/baseline/` files.
