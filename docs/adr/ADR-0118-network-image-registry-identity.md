# ADR-0118: Network image identity includes its registry

- **Status:** Accepted
- **Date:** 2026-10-04
- **Related:** ADR-0105 (asset runtime ownership), ADR-0107 (asset byte sources)

## Context

An asset registry owns its HTTP client, including caller-configured headers,
authentication, proxies and redirects. A URL alone therefore does not identify
the response: two registries can legitimately receive different images from the
same URL. The widget decode cache and pending-load map previously shared both
completed responses and in-flight work across those owners.

## Decision

`NetworkImage` identifies a response by its URL and the allocation identity of
its `Arc<AssetRegistry>`. Cloned providers using the same registry share cached
images and pending work. Different registries never share either, even when
their URL strings and client configurations match.

The opaque `NetworkImageKey` holds a `Weak<AssetRegistry>`. Equality uses
`Weak::ptr_eq`; hashing uses the same pointer identity and URL. Keeping the weak
reference prevents that allocation address from being reused while a key is
retained, without retaining the registry's client or background runtime. A key
whose owner has gone remains isolated; it never falls back to URL equality.
No process-wide identity counter or counter-exhaustion policy is needed.

Registry HTTP configuration accepts a `reqwest::ClientBuilder`, creates a fresh
client fallibly during configuration, and establishes connections only on the
bridge's selected loading runtime. Accepting a previously used `NetworkLoader`
could import idle connections whose driver belongs to a live but undriven
current-thread runtime. Cloning that client shares the same dependency rather
than repairing it. The builder boundary excludes that state by construction;
the default client remains lazily initialized on the loading runtime.

The raw `NetworkLoader::with_client` API still permits an explicitly shared
client. Its caller must keep the runtimes driving that client's connections
driven when using it across runtimes. It does not promise registry-style runtime
selection.

Asset-file keys retain their existing path identity. This decision concerns
HTTP response authority and does not redesign the decoded-image cache's
storage, eviction or subscription lifetime.

## Consequences

`ImageCacheKey::Network` contains `NetworkImageKey`, replacing its public
`String` payload, and exists with the `network-images` feature. Consumers obtain
the opaque key through `ImageProvider::cache_key`. There is no supported way to
forge a registry identity from a URL or address.

`network_bridge_reuses_connections_and_recovers_after_decode_errors` verifies
fallible host configuration, preserved redirect policy through capacity changes,
and a configured client built while an undriven current-thread runtime is entered.

A strong registry reference in the key would retain its runtime through cached
images. A bare address could alias a later allocation. Comparing client labels
or settings cannot establish ownership identity. The weak allocation identity
avoids all three cases.

`network_images_scope_cached_and_pending_responses_to_the_registry` exercises
two real PNG responses with different dimensions at one URL, sequential cached
loads, concurrent pending loads, same-registry response coalescing and owner release
while a cache key remains live. Restoring URL-only identity makes the decoded
dimensions assertion fail.
