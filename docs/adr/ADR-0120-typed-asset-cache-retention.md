# ADR-0120: Typed asset-cache retention and shared initialization

- **Status:** Accepted
- **Date:** 2026-10-04
- **Related:** ADR-0105 (asset runtime ownership), ADR-0107 (asset byte sources)

## Context

An asset cache stores arbitrary decoded `Asset::Data` through shared ownership.
A caller's byte capacity was divided by a guessed 10 KiB per entry and raised to
at least 100 entries. This neither bounded memory nor honored small capacities.
The public cache initializer duplicated concurrent loads even though registry
loads already used Moka's coalescing. Presence checks also counted as retrievals.

## Decision

`CacheCapacity` explicitly selects disabled completed-entry retention or a
nonzero `u64` entry limit. The default is 10,240 entries per asset type, preserving
the previous default's actual count. Consumer handles retain data independently
of capacity and expiration; no process-memory or decoded-byte bound is promised.

`AssetCacheConfig` carries capacity and independent lifetime and idle expiration.
Defaults remain five minutes and one minute. `CacheExpiration::NEVER` disables
one expiration rule; its checked constructor admits zero through 1,000 years of
365 days and rejects longer durations before Moka's builder can panic.

Moka's default TinyLFU admission and LRU eviction remain appropriate for concurrent
typed asset loading. `AssetCache::get_or_insert_with` and registry loading share
the borrowed-key fallible entry initializer. Concurrent callers receive the same
allocation or shared `Arc<Error>`; failures are not retained. A waiter can retry
after an initializer is cancelled or panics, subject to Moka's retry limits.
Registry loading continues to return an owned `AssetError` and validate before
cache lookup. Successful initializers must be safe to restart after cancellation.

Coalescing covers independent requests. An initializer's same-key reentry through
a clone uses completed data if present, otherwise returns independently initialized
uncached data; its outer initializer retains publication ownership. Poll-scoped
ancestry belongs to the typed cache, and no infrastructure guard spans user work.
This includes reentry after suspension, but cannot identify cycles through
separately spawned tasks. The public cold-load family pins reentry and recovery.

Presence is a synchronous observation using `contains_key`: it records no
retrieval, clones no data, and changes neither popularity nor idle expiration.
Counts remain `u64` estimates. Utilization divides the estimated count by the
configured limit, clamps maintenance lag to one and reports zero when disabled.
Statistics describe operations: initial presence probes classify initializer
requests; completed fresh results count insertions. They are not linearizable
residency or physical-release metrics. Explicit invalidations are named
`invalidations`, including absent keys; automatic evictions are not counted.

Invalidating completed entries does not cancel an initializer that can publish
afterward. `clear_all` instead detaches the registry's entire cache map under its
guard, then retires it after releasing the guard. Generic data destructors can
reenter the registry and create fresh caches. Detached in-flight operations may
still return handles. Retirement introduces no panic-containment guarantee for
user destructors, including aggregates that double-panic.

## Consequences

This is a breaking change to capacity configuration, cache initializer error
ownership, synchronous presence, statistics naming and count width. It removes
the guessed-byte contract rather than adding a misleading generic weigher.
A future byte policy requires an explicit trustworthy asset-weight contract.

The widget image bridge directly loads images and bypasses typed registry caching.
Its completed-image LRU supports synchronous frame-path probes and its own
subscription ownership. Moka's asynchronous retrieval cannot replace that boundary.
Explicit callers can still populate both caches; no sharing of their allocations
or combined memory budget is promised.

`asset_cache_retention_and_observation_contracts` pins small and disabled capacity,
coalescing followed by reload, finite metrics and checked expiration.
`cold_registry_loads_share_work_and_recover` exercises shared non-Clone errors,
cancellation, panic and subsequent retry through public handles.
`cache_clones_report_shared_operations_and_reset` checks observational presence
and shared counters.
`registry_cache_retirement_commits_ownership_before_data_drop` exercises reentry
through a live registry and the absent-owner fallback after last-owner release.
