# Moka API audit, 2026-10-04

## Sources and version decision

Reviewed [Moka 0.12.16](https://docs.rs/moka/0.12.16/moka/),
[future Cache](https://docs.rs/moka/0.12.16/moka/future/struct.Cache.html),
[CacheBuilder](https://docs.rs/moka/0.12.16/moka/future/struct.CacheBuilder.html),
[borrowed entry selectors](https://docs.rs/moka/0.12.16/moka/future/struct.RefKeyEntrySelector.html)
and owned selectors/Entry, alongside the published crate's source, feature manifest
and exact [upstream changelog](https://github.com/moka-rs/moka/blob/a616ec19e8d4ed938caf8b2c88090331d778d5da/CHANGELOG.md).

The lockfile already selects 0.12.16; the workspace minimum now requires it.
Its insertion/removal race fix matters even under the default TinyLFU policy:
earlier versions could retain phantom slots, over-report counts and progressively
lose usable capacity. Under Moka's optional LRU policy, that race could stall
eviction entirely. The same release raises crossbeam-epoch's advisory-safe minimum
and repairs a ThreadSanitizer false positive; Moka itself does not exercise the
advisory's pointer-formatting path. Version 0.12.15 also repaired expired-entry
reinsertion under custom Expiry, which FLUI does not enable.

## API-family decisions

| Family reviewed | FLUI decision |
| --- | --- |
| Cache clone, name, policy, new/builder | Clones share data and counters; read real policy capacity. No unused naming surface. |
| contains_key, get, borrowed keys and Equivalent | Presence must be observational; retrieving reads update policy. Asset handles own their keys, so retain the typed key contract. |
| get_with/by_ref/if, optionally_get_with/by_ref, try_get_with/by_ref | Loading is fallible; use borrowed fallible entry initialization to additionally inspect freshness. No negative caching or automatic refresh contract. |
| entry/entry_by_ref, or_default, or_insert, or_insert_with/if, optional/fallible insert | Borrowed or_try_insert_with coalesces cold work and shares non-Clone errors. Owned selectors clone keys earlier without benefit here. |
| and_compute_with, and_try_compute_with, and_try_compute_if_nobody_else, and_upsert_with | Serialize per-key transformations; FLUI has immutable asset snapshots, not a mutable/refreshing asset contract. Avoid unwired update APIs. |
| Entry key/value/into_value, is_fresh and replacement metadata | Fresh returned results count completed insertions; warm hits and waiting cold callers are distinguished by the initial presence probe. |
| insert, invalidate, remove, invalidate_all | Keep explicit insertion/invalidation. Remove clones a discarded value and is unnecessary. Invalidation neither cancels pending loads nor releases all consumer ownership. |
| invalidate_entries_if, PredicateId/Error | Requires extra builder state and callback lifecycle. No production predicate invalidation need; leave disabled. |
| iter/IntoIterator | Weak concurrent iteration does not promote keys; unnecessary for existing cache operations. No snapshot guarantee invented. |
| entry_count, weighted_size, run_pending_tasks, debug_stats | Entry estimates and maintenance are useful. Debug stats are feature-gated unstable instrumentation; no dependency on them. No byte claim from unweighted size. |
| max/initial capacity, weigher, eviction_policy | Explicit count bound; preserve TinyLFU default. No guessed 10 KiB weight, arbitrary preallocation or switch to Moka LRU. A trustworthy per-data weight contract must precede weighting. |
| time_to_live, time_to_idle, expire_after/Expiry | Expose bounded lifetime/idle intervals and explicit absence. No caller requirement for custom per-entry callbacks. Check Moka's 1,000-year limit before build. |
| eviction_listener, async_eviction_listener, RemovalCause | Listeners receive owned clones, may run during maintenance, and are disabled after a panic. Rename request counters accurately rather than adding an unnecessary callback. |
| build/build_with_hasher, hashing | Keep RandomState for externally influenced asset keys. Faster hashing is not justified by measurements. |
| sync/SegmentedCache, future, logging, quanta, atomic64, unstable-debug-counters | Only future is selected. Sync caches cannot provide asynchronous cold-load coalescing. Other features are unnecessary; atomic64 is a compatibility no-op. |

Read/write recording and maintenance are eventually consistent. Writes can await
maintenance capacity; the wrapper also uses locks for statistics and registry maps.
This is not a completely lock-free API. Arc values avoid cloning decoded payloads.
Moka initializes per key and error type; failed initializers are not stored.
Cancellation or panic allows another caller's initializer to retry, with a bounded
upstream retry policy. Callers must tolerate restart and must not treat clear as
cancellation. Generic destructor retirement is outside the registry lock, without
a new guarantee of containing panicking destructors.

## Consumer coverage ledger

| Consumer | Reading and changes | Compilation / execution |
| --- | --- | --- |
| flui-assets cache/config/stats | Count capacity, checked expiration, shared public initialization, presence, metrics and retirement contracts | All-target Clippy in 10 feature configurations; full-feature 14 tests and no-default-feature 8 tests passed on Windows |
| flui-assets registry | Unified initialization preserves validation and owned AssetError; builder configuration preserves HTTP/runtime owners | All-target Clippy in 10 feature configurations; full-feature 14 tests and no-default-feature 8 tests passed on Windows |
| flui-assets public tests | Count/disabled/expiration, shared allocation/non-Clone error, cancellation/panic/retry, observational counters and bounded destructor reentry | Final full-feature suite 14 passed; six production controls failed intentionally |
| flui-assets images/network/default/full | All declared optional paths, test modules and library examples reviewed | 10 feature configurations compiled; default/full tests executed; full-feature doctests 16 passed and 50 ignored |
| flui-assets documentation | README, GUIDE, PERFORMANCE, PATTERNS and architecture; corrected obsolete byte hints and unsupported claims | Strict source gates passed |
| widget image cache / bridge | Synchronous LRU probe and subscription policy; bridge calls direct image load and bypasses typed caching | Documentation corrected; dependent whole-workspace Clippy and 652 tests passed |
| root manifest/tooling | Future-only Moka minimum, locked version already current; no shader or benchmark directly consumes Moka | deps --strict and whole-workspace check-changed passed |

Moka stores typed data, including decoded images. The widget bridge bypasses that
cache and hands completed decodes to a synchronous LRU. Explicit independent
registry and widget loads can hold separate allocations. There is no combined
memory bound or universal sharing guarantee. Keep the distinct ownership and
frame-path boundaries.

## Regression evidence and limits

Public tests exercise production producers, not copies of policy predicates.
Restoring map destruction under the registry guard makes destructor reentry
time out. Restoring check/load/insert makes concurrent loaders run twice; restoring
the minimum of 100 entries violates the two-entry test. Presence counter pollution
is separately restored as a narrow control of the old observational defect.
Each control runs serially, preserving exact production bytes in a finally path.

Wrong historical utilization and overflowing integer-rate arithmetic also fail their real consumer cases. Whole-workspace check-changed passed: 652 tests passed/10 skipped, strict private rustdoc, 619 doctests passed/388 ignored, available Windows/macOS/WASM compilation. flui-assets itself was compiled and executed on Windows; the broader cross-target gates do not establish another platform execution for Moka. assets_basic_usage ran successfully with full features (font allocation sharing, decoded image, invalidation and preload). Android/iOS targets and the Linux xvfb platform suite were unavailable locally. No benchmark,
interactive example, absent-target runtime or general memory-performance claim
follows from reading or compilation.
