### Changed

- `TransferActions` and `PointerKindSet` (`flui-platform`) are the contract's own flag sets
  instead of `bitflags` types: the same constants, `empty`, `all`, `bits`, `from_bits`,
  `contains`, `union`, `insert`, `remove`, `set`, `iter_names`, `|`, `|=`, `&` and the `Debug`
  form stay, and `iter_names` returns an `impl Iterator`; the rest of `bitflags`' API (`iter`,
  `intersects`, `-`, `!`, …) and its `bitflags::Flags` implementation are gone
  ([ADR-0089](/docs/adr/ADR-0089-upstream-types-in-stable-signatures.md) §2).

### Added

- `cargo xtask api-closure`: what a Stable crate's public API names from upstream crates, read from
  nightly rustdoc JSON and checked against ADR-0089 §2 in CI's `doc` job, with a `--self-test`;
  the job also reports `cargo semver-checks` for `flui-platform`.
