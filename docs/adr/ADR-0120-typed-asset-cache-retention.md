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

Moka's default TinyLFU admission and LRU eviction remain appropriate for
concurrent typed retention. Generic public initialization performs get, load
and insert without awaiting pending same-key work. Each successful invocation
publishes; a late completion may replace earlier data while its returned handles
remain valid. Errors stay owned and are not retained. Cancellation and panic leave
independent work unaffected and later calls free to retry. No exactly-once loading
or side-effect guarantee is made. This permits direct and awaited native spawned
reentry without runtime-specific context or inference of arbitrary wait graphs.

Registry admission validates every descriptor. Custom Asset implementations use
that independent path and return owned AssetError values. Only the exact built-in
FontAsset type enters the private Moka borrowed-key fallible initializer. Its
closed producer reads bytes through standard file IO and wraps them in Arc,
without custom asset or decoder callbacks. ImageAsset preserves registered
image decoder hooks, which can reenter, and uses independent initialization. A wrapper or custom Asset cannot opt in;
TypeId checks do not cast data or duplicate loader implementations.

Font same-key contenders share one allocation or failure; errors are not
cached. Moka waiters retry after elected cancellation or panic subject to its
finite retry limit. These producers must tolerate restart. No infrastructure
guard spans arbitrary user initialization or retirement.

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

This is a breaking change to capacity configuration, custom registry cold-load
behavior, synchronous presence, statistics naming and count width. It removes
the guessed-byte contract rather than adding a misleading generic weigher.
A future byte policy requires an explicit trustworthy asset-weight contract.

The widget image bridge directly loads images and bypasses typed registry caching.
Its completed-image LRU supports synchronous frame-path probes and its own
subscription ownership. Moka's asynchronous retrieval cannot replace that boundary.
Explicit callers can still populate both caches; no sharing of their allocations
or combined memory budget is promised.

`asset_cache_retention_and_observation_contracts` pins small and disabled capacity,
independent loading followed by reload, finite metrics and checked expiration.
`cold_registry_loads_preserve_publication_and_recover` exercises independent owned
non-Clone errors and spawned child progress,
cancellation, panic and subsequent retry through public handles.
`cache_clones_report_shared_operations_and_reset` checks observational presence
and shared counters.
`registry_cache_retirement_commits_ownership_before_data_drop` exercises reentry
through a live registry and the absent-owner fallback after last-owner release.

The generic helper retains its pre-audit independent publication behavior and
owned error type. Custom registry cold loads change from universal singleflight
to independent work because arbitrary Asset::load can await same-key children.
Font producers retain singleflight; image/custom producers remain independent. The private
`builtin_initialization_shares_work_and_recovers` matrix pins built-in failure
sharing and waiter recovery; public font/image rows prove shared fonts and independent images.

`registered_image_hook_can_await_same_key_spawned_work` pins the actual callback
boundary: registered decoding is preserved, an awaited same-key child finishes,
its successful result publishes, parent replacement preserves the child handle,
and a subsequent cache hit performs no decoding. Its hook registrations run in
an isolated test process. Built-in-only image decoding is not introduced here.
