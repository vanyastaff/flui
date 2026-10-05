# Lasso API and asset-name ownership audit

## Sources and declared scope

Reviewed Lasso 0.7.3, the locked and latest published release at audit time:
[crate documentation](https://docs.rs/lasso/0.7.3/lasso/),
[ThreadedRodeo](https://docs.rs/lasso/0.7.3/lasso/struct.ThreadedRodeo.html),
and the published source at
[upstream commit 85aacda](https://github.com/Kixiron/lasso/tree/85aacda9a22798961b868af127bf5bcd00cff5e5).
The registry package's `.cargo_vcs_info.json` identifies that commit. The
published manifest and implementation were read to check feature availability,
allocation ownership, insertion failures and key-domain assumptions.

The upstream [release history](https://github.com/Kixiron/lasso/blob/85aacda9a22798961b868af127bf5bcd00cff5e5/Changelog.md)
was also read: 0.7.3 updates MSRV and hash-map dependencies; 0.7 adds the lock-free
string arena and fallible cloning; earlier releases add arena memory limits,
readers/resolvers and static interning. None adds individual-name reclamation to
`ThreadedRodeo`. Historical release notes are not a substitute for the current
fallible/infallible method signatures in the published source.

The only direct workspace consumer was `flui-assets`, with `multi-threaded`
enabled. Its `AssetKey` used a process-global `ThreadedRodeo` and `Spur`; font
and image descriptors constructed keys from names. The audit includes that
consumer's source, optional features, tests, examples, documentation, manifests,
global-state exception and Cargo-generated lockfile. No shader or tooling source
used Lasso. Other crates consume the asset contract rather than Lasso directly.

## API-family decisions

| Family | What was reviewed | Decision for FLUI |
| --- | --- | --- |
| Mutable and concurrent interners | `Rodeo`, `ThreadedRodeo`, constructors, custom hashers, capacities and memory limits | Useful for an explicitly scoped dictionary; a process-global dictionary cannot release individual dynamic asset names. Remove it from asset identity. |
| Interning and lookup | `get_or_intern`, fallible and static variants, `get`, `contains`, `contains_key` | Interning deduplicates names within one arena. Static insertion avoids copying genuinely static strings, but does not give dynamic file names an owner-controlled lifetime. |
| Resolution | `resolve`, `try_resolve`, unchecked resolution where exposed | Keys belong to their issuing dictionary. An in-range numeric key from another dictionary can resolve to an unrelated name; bounds checking does not establish domain ownership. Owned string keys avoid this domain protocol. |
| Key representations | `Spur`, `LargeSpur`, `MiniSpur`, `MicroSpur`, `Key` round-trip contract and integer projections | Compact IDs are valuable inside a scoped arena. The asset API needs independently constructed equal names and releasable ownership, rather than portable-looking arena indices. Remove `as_u32`. |
| Frozen dictionaries | `into_reader`, `into_resolver`, reader/resolver traits and contention-free lookup | Freezing stops mutation and improves reads, but retains the dictionary's entire allocation. It cannot release one asset name while other names remain live. |
| Mutation and collection | Mutable `clear`, cloning, iteration, strings, extension and collection construction | `Rodeo::clear` invalidates the meaning of old keys and permits slot reuse. `ThreadedRodeo` provides no individual-name removal. Neither implements the required asset lifetime. |
| Metrics and failures | Length/capacity, current/max memory usage, changing limits, key-space exhaustion, memory-limit and allocation errors | Arena limits govern string storage, not all map/process memory. Initial capacity can affect the effective limit. Insertion can allocate string storage before discovering key exhaustion. Fallible insertion is admission control, not reclamation. |
| Optional features | `multi-threaded`, `serialize`, `inline-more`, `ahasher`, `no-std`, `abomonation`, `deepsize` | Only concurrency was enabled here. Serialization and introspection do not change ownership. The published source excludes `ThreadedRodeo` under `no-std`; do not infer support from a broad documentation table. No extra feature improves the asset contract. |
| Performance | Numeric comparisons, hashing choice, mutable/concurrent/frozen lookup tradeoffs and upstream benchmarks | Interning trades insertion and retained storage for compact equality/hash operations. Historical upstream benchmarks are not FLUI measurements. No speed or memory-budget claim is made for the replacement. |

## Resulting contract

`AssetKey` owns a nonempty `Arc<str>` and compares/hashes string contents.
Clones share storage; independently constructed equal names remain equal but
may have distinct allocations. Built-in font/image descriptors also hold shared
names and hand the existing allocation to keys. There is no retaining global
arena. `as_str` borrows from the key instead of returning a static reference.

This is deliberately breaking: keys implement `Clone`, not `Copy`; integer
projection is gone; constructor inputs for built-in descriptors are
`Into<Arc<str>>` (`&String` callers use `.as_str()`). A weak data handle still
owns its key and therefore its name. The final descriptor/key/handle owner
releases name storage, independently of other live names. Data-cache eviction
alone cannot promise release while consumers retain keys.

Shared owned keys are larger than `Spur`, and equality/hash read string contents.
This is an ownership repair, not a demonstrated throughput optimization. A
scoped interner could be reconsidered with an explicit domain and lifetime
contract if measurements justify it. The cross-crate decision is recorded in
[ADR-0121](../adr/ADR-0121-owned-asset-key-names.md), implementing ADR-0097's
proposed removal of the asset interner.

## Coverage and evidence

| Surface | Read and changed | Compilation | Execution |
| --- | --- | --- | --- |
| `flui-assets` keys, font/image producers, handles and registry | Complete direct-consumer review; owned-name wiring and public contract family | All ten feature-powerset configurations, all targets, Clippy with warnings denied | Full-feature suite: 15 passed; default suite and example verified below |
| Optional images/network paths | Read producer/error paths; image producer uses shared names | Included in feature-powerset checks | Image-name row runs with full features; network-only and images-only are compile coverage |
| Public docs/examples | README, guide, patterns, performance, architecture, crate/type docs; migration and cost documented | Full-feature doctests | 16 passed, 50 ignored; ignored examples are not execution evidence |
| Workspace consumers | Searched asset-key usage; no direct Lasso use outside assets | Workspace gate recorded below | Host execution only; alternate platform typechecks are not runtime evidence |
| Manifests/global exception/lockfile | Removed both Lasso declarations and the `INTERNER` exception; Cargo regenerated lockfile | Dependency and source gates recorded below | No new dependency or global introduced |

The public `asset_key_names_follow_consumer_ownership` table exercises real font
and image descriptors through the registry. Independently allocated equal names
share loaded data; distinct names do not. It follows storage through descriptors,
cache hits, lookup, strong clones and weak data handles, then observes final name
release while another name remains live.

Two serial production controls restored the relevant defects, with exact source
bytes restored in `finally` before subsequent builds:

- Retaining an extra name owner permanently makes both producer rows fail the
  final-name release assertion. This restores the old retention defect without
  recreating the old representation.
- Returning a leaked static name makes the `as_str` compile-fail doctest compile
  successfully, and the doctest runner rejects it for that intended reason.

Local evidence logs are under the ignored worktree target directory:
`lasso-initial-tests.log`, `lasso-feature-matrix.log`, `lasso-doctests.log`,
`lasso-control-retention.log` and `lasso-control-lifetime.log`.
Dependency and source gates passed (cargo xtask deps --strict and checks --strict).
After rebasing onto the repaired Moka initializer code, cargo xtask check-changed
--base codex/moka-api-audit completed successfully: whole-workspace/all-target
Clippy plus the engine testing feature, 653 nextest tests passed/10 skipped,
strict private rustdoc, and 619 doctests passed/388 ignored. Available Windows,
macOS and WASM compilation passed. The asset-name implementation and all public
behavior tests were unchanged by the subsequent documentation-only cache ADR
renumbering; final strict documentation/source checks validate those links.

Windows is the execution host. No alternate-platform Lasso execution, benchmark,
interactive example or public-network example is claimed. The workspace WASM
lane excludes `flui-assets`; Linux/xvfb execution and Android/iOS targets are not
available locally.
