# ADR-0121: Asset names have shared ownership

- **Status:** Accepted
- **Date:** 2026-10-04
- **Implements:** ADR-0097's proposed removal of the asset interner
- **Related:** ADR-0107 (asset sources), ADR-0120 (typed cache retention)

## Context

AssetKey held a Lasso Spur into a process-wide ThreadedRodeo. Every distinct
name remained allocated permanently, independent of cache expiration, registry
ownership and consumer handles. Integer keys also required that global domain
to resolve names. There is no measured framework workload that requires
process-wide constant-time interning to justify those lifetimes.

Lasso 0.7.3 provides concurrent interning and bounded string-arena admission.
It cannot remove individual entries from ThreadedRodeo. Frozen readers and
resolvers retain the entire arena and stop dynamic insertion. A scoped interner
would require every surviving key to retain its domain; one key would still
retain unrelated names. Clearing a mutable arena would invalidate live numeric
keys and permit index reuse.

## Decision

AssetKey owns a nonempty Arc<str>. Clones share the existing allocation, while
independently constructed keys compare and hash by name contents. Names identify
equivalent requests within an asset type and registry; they are not backend
allocation identities. No global table or numeric projection is involved.

The string borrowed by as_str is tied to the key's lifetime. From<Arc<str>>
preserves caller-owned shared storage and checks the same nonempty invariant as
other constructors. Built-in font and image descriptors own shared names and
clone that ownership when producing keys, instead of copying the name per lookup.
Their file and embedded constructors accept Into<Arc<str>>; String, string slices
and shared names remain supported. A borrowed String uses its as_str adapter.

Strong and weak data handles both own their keys. Releasing a registry or the
last data owner therefore does not necessarily release the name. String storage
is released when its final name owner goes; unrelated live names do not retain
it. Data and name ownership are distinct and remain valid across registry teardown.

The Lasso dependency and the asset interner's global exception are removed.
ADR-0097's global-state policy remains unchanged.

## Consequences

AssetKey implements Clone without Copy. Its as_u32 projection is removed, and
as_str no longer supplies a static borrow. Cloned names avoid copying their
contents, but keys are larger than the former numeric Spur and hashing/equality
can examine the string's contents. No benchmark speedup or combined memory budget
is promised. Independently constructed equal strings may use different storage;
only clones and explicitly shared inputs promise allocation sharing.

The public asset_key_names_follow_consumer_ownership family loads real fonts and
images through the registry, verifies equal-name cache identity and distinct-name
separation, releases the registry, and observes string ownership through strong
and weak data handles. Final release must succeed while another name remains live.
A compile-fail doctest on AssetKey::as_str prevents a borrow from escaping its owner.

Regression controls restore permanent ownership of shared names and an escaping
static borrow separately. These narrowly recreate the old retention and borrowing
defects while preserving compilation of the new API. Each must fail for its
intended contract, with exact production bytes restored afterward.
