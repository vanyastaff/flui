# Design patterns in flui_assets

## Application-owned registry

`AssetRegistry` is an ordinary value normally shared as `Arc<AssetRegistry>`.
Its cache, pending loads, byte budget and decode slots belong to that value, not
to a process or host singleton. Equal sources in two registries are independent.

The builder has no type-state capacity transition:

```rust
use flui_assets::AssetRegistryBuilder;

let registry = AssetRegistryBuilder::new().build();
```

This is deliberate: decoded pixels already have a trustworthy weight, so no
caller must choose an arbitrary per-type entry count before constructing a
registry.

## One completed-image store

The registry's LRU is the only completed-image cache. `load_image` and
`load_network_image` populate it, and `cached_image` probes it synchronously.
There is no parallel typed cache or widget-global LRU that could disagree about
identity, retention or memory pressure.

A cached `Image` shares ownership with its caller. Eviction removes registry
ownership; it does not invalidate a value already returned.

## Typed source identity

`ImageSource` distinguishes asset paths from network requests. A network source
contains both its URL and the fetching client's `HttpAuthority`. This prevents
one URL served by two clients with different content from coalescing or cache
sharing incorrectly.

Ticket allocation identity distinguishes generations of an in-flight load. An
old load finishing after cancellation or replacement cannot complete the newer
entry.

## Subscriber-owned shared futures

Every `ImageLoad` is one subscriber to registry-owned work. The work survives
while at least one subscriber remains and is cancelled when the last leaves.
This is stronger than cancelling whichever caller happened to start the load,
and avoids orphaned file reads, requests and queued decodes.

A run dropped unfinished closes rather than fabricating a decode error. A
remaining subscriber can become the new lead on its own services. A real failure
produces a typed error and is not retained.

## Execution as a caller capability

`LoadServices` carries execution lanes and, when needed, HTTP. The registry
does not select a global runtime:

- read/fetch on IO;
- decode on compute;
- wait for compute-lane room without occupying the IO worker;
- with no lanes, run synchronously where the future is polled.

This keeps UI frame code synchronous and makes headless tests independent of an
ambient Tokio reactor.

## HTTP as a separate capability

The registry needs an `Http` client, not a concrete networking stack. Hosts and
tests provide that client through `LoadServices`. The crate therefore has no
`network` feature, connection pool or TLS backend. Missing HTTP is an immediate
`HttpUnavailable` result, not a runtime stub that first performs other work.

## Bounded work queues

Admission and decode reservations are separate. Four load slots per decode
thread bound reads/fetches and encoded bytes waiting for decode. Decode slots
go to the earliest admitted load whose bytes are ready, avoiding head-of-line
blocking behind a slow response. Cancelled reservations release their place
without running user decoding.

Locks protect state transitions, not user code. Wakers, lane submission,
completion sends, cancellation and pixel destruction happen after internal
locks are released.

## Memory-pressure ownership

`ReclaimMemory` acts on decoded pixels only. The application explicitly
registers the registry it owns. Renderer textures and caller-held images have
their own lifetimes and are not hidden behind the same operation.

## References

- [Architecture](ARCHITECTURE.md)
- [User guide](GUIDE.md)
- [Performance and memory](PERFORMANCE.md)
- [ADR-0317](../../../docs/adr/ADR-0317-the-asset-registry-is-image-only-and-the-facade-opts-into-it.md)
