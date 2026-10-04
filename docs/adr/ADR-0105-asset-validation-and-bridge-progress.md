# ADR-0105: Asset validation and bridged load progress

- **Status:** Accepted
- **Date:** 2026-10-03
- **Related:** [ADR-0018](ADR-0018-async-builder-seam.md),
  [ADR-0047](ADR-0047-unified-execution-services.md),
  [ADR-0097](ADR-0097-no-process-global-state-gate.md)

## Context

`Asset::validate` advertises validation before loading, but the registry previously
never called it. Returning a cached result also silently accepted a descriptor whose
validation rejected it. The asset bridge returns a reactor-free completion future
for the frame-driven scheduler, but entering an ambient current-thread Tokio runtime
does not drive tasks spawned on it. Such a bridge could remain pending indefinitely.
Starting the registry's fallback runtime can fail because OS resources are unavailable.

## Decision

`AssetRegistry::load` validates the supplied descriptor before computing its key,
accessing a cache, or invoking its loader. Validation rejection is authoritative on
both misses and hits. It does not evict data previously loaded through an accepted
descriptor; `get` remains a cache lookup without a descriptor to validate. Direct
`Asset::load` callers remain responsible for invoking validation when needed.

Bridged image loads use an explicitly injected runtime first, then an ambient
multi-thread Tokio runtime, then a registry-owned single-worker runtime. Ambient
current-thread runtimes are skipped even when currently driven: the bridge cannot
rely on their continued progress when its completion future is polled by FLUI.
Injection is an explicit host ownership promise: the runtime must remain alive and
driven until loads finish. A current-thread handle can be injected when the host
drives its runtime with `Runtime::block_on`; entering the handle alone is insufficient.
Runtime-neutral host execution services remain the separate contract of ADR-0047;
this decision does not add an executor singleton or move runtime ownership upward.

Owned runtime creation is serialized in a registry-local infrastructure mutex.
Construction errors become `AssetError::Io` through the same completion channel
as loading errors. Failure leaves the owned runtime absent, so the next operation
can retry. Successful initialization is reused, and teardown continues to use
`shutdown_background` so dropping a registry from a Tokio task does not block.

## Behavioral evidence

- `validation_precedes_loading_and_cache_hits_without_poisoning_accepted_data`
  rejects misses before loading/caching, rejects hits, preserves accepted cached
  data, and accepts the next request without reloading.
- `load_image_bridged_completes_both_the_success_and_the_failure_path` decodes the
  committed PNG with a reactor-free executor, reports a missing file and then
  decodes successfully using the same registry. It repeats recovery while a
  current-thread runtime is entered but never driven; the poll loop has a deadline.
- `the_bridge_resolves_a_live_runtime_and_survives_its_own_teardown` includes a
  private runtime-construction failure seam, asserts the typed error and proves
  a subsequent successful initialization actually executes a task. It also checks
  explicit injection, stale ambient handles, and teardown inside an owned task.

## Consequences

Validation can reject an asset descriptor even when its key already has cached
data. Ambient current-thread applications use a background worker for bridged loads
unless they explicitly inject a driven runtime. Resource exhaustion is a recoverable
operation error rather than an internal-invariant panic. Unsupported generic loader
interfaces and byte-source unification are separate decisions and remain unchanged.
