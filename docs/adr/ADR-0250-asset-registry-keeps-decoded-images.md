# ADR-0250: The asset registry keeps decoded images and schedules their decodes

- **Status:** Accepted
- **Date:** 2026-10-19
- **Amends:** [ADR-0097](ADR-0097-no-process-global-state-gate.md) (removes the decoded-image
  `CACHE` global), [ADR-0118](ADR-0118-network-image-registry-identity.md) (registry identity now
  covers asset images too, and the key is opaque),
  [ADR-0237](ADR-0237-window-state-and-host-notices.md) decision 9 (which image memory the host
  reclaims itself)
- **Related:** [ADR-0105](ADR-0105-asset-validation-and-bridge-progress.md) (bridge runtime
  selection, unchanged)

## Context

`flui-widgets` kept decoded images in a process-wide LRU of 100 entries with in-flight load
coalescing, and the asset registry's bridge spawned one task per load that read, decoded inline on
an async worker, and sent the result down a oneshot. Five defects followed from that shape:

- No memory-pressure notice could reach the cache: it was a static with no receiver, and a few
  4K photos in 100 entries are gigabytes.
- Dropping the last waiter removed the pending entry but not the task: an abandoned load still read
  and decoded, and an abandoned HTTP request held its connection until the server answered.
- Decodes ran in spawn order on the registry's single worker, so after a fling the visible images
  waited behind every abandoned one.
- On a host's own multi-thread runtime, decodes occupied its async workers.
- Asset images were keyed by path alone, so two registries shared pixels for equal names while
  network images were already scoped to their registry (ADR-0118).

## Decision

1. **The registry keeps the decoded images of the loads it bridges.** `AssetRegistry` owns an LRU
   bounded by pixel bytes (`AssetRegistryBuilder::with_decoded_image_budget`; 256 MiB, 64 MiB on
   Android, iOS and the web), a pending map, and `cached_image(&ImageSource)` as the synchronous
   probe. An image larger than the budget is handed to its waiters and not kept. There is no
   process-wide image cache; the `image::decode_cache::CACHE` global and its allowlist entry are
   removed.
2. **A load lives as long as someone waits for it.** Every `ImageLoad` waiting on a pending load
   is a subscriber. The last one's drop removes the entry, cancels the load's ticket and aborts its
   task at its next await: an HTTP request is dropped at once, a file read (in 256 KiB chunks,
   one blocking read each) stops at the next chunk, and a decode that has not started never runs.
   A file open or a chunk already on a blocking thread cannot be interrupted and finishes. The ticket's
   allocation is the load's identity; a task completing after its entry was abandoned or replaced
   leaves the newer entry alone. A task that panics or is dropped by its runtime settles its entry
   from a drop guard and closes its waiters.
3. **Decodes run on the loading runtime's blocking pool behind slots.** The decode runs in
   `spawn_blocking`, at most `with_decode_threads` at once (half the cores, at most four). A
   host's async workers never decode. Before reading, a load takes one of four load slots per
   decode thread and holds it until its decode returns, which bounds the reads in flight and
   the encoded bytes waiting to decode by count. Once its bytes are read it asks for a decode
   slot. Load slots are granted in admission order. A decode slot goes to the earliest-admitted
   load whose bytes are ready, and a load still fetching holds none: the decoder is
   work-conserving, so a ready load admitted later decodes rather than waiting idle behind a
   slow response. Strict admission order would let one stalled request block every visible
   image behind it. A stalled request holds its load slot until the loader's read
   timeout or its cancellation. Priority among live loads is admission order: an image
   scrolled out of a lazy list unmounts and cancels, so the loads left waiting are the
   mounted ones.
4. **Keys are typed.** `flui_widgets::ImageCacheKey` is opaque: built-in providers key by the
   registry's weak allocation identity and the `ImageSource`; custom providers key by a value of
   their own type through `ImageCacheKey::custom`. `ImageProvider::cached` is the non-blocking
   probe `Image` makes before subscribing.
5. **Memory pressure.** The renderer reclaims image textures itself: moderate pressure evicts
   those the last frame did not draw, critical pressure evicts all and releases the image atlas texture. The
   registry's `ReclaimMemory` trims decoded images to half the budget (moderate) or drops them
   (critical). The application owns its registry and registers it with
   `AppConfig::with_memory_reclaim`, as it already registers the registry's typed caches; the host
   does not own a registry it could reclaim unasked.

## Consequences

- `load_image_bridged` and `load_network_image_bridged` return `ImageLoad` and consult the
  registry's decoded-image cache; a second load of a decoded source is a hit, not a request.
- `ImageCacheKey::Asset`, `ImageCacheKey::Network` and `NetworkImageKey` are gone; a custom
  provider that used `ImageCacheKey::Asset(label)` moves to `ImageCacheKey::custom(value)`.
- `NetworkLoader::new` has connect and read timeouts, and `load_url` refuses bodies over 256 MiB.
- An application that does not register its registry keeps its decoded images under pressure.
  Removing that opt-in needs host-owned asset services, which is a separate decision.
- `Image::file` and `Image::memory` still decode on the owner thread; their providers take no
  registry and so have no decode pool to reach.

## Verification

- `decoded_images_stay_within_their_byte_budget_and_give_back_under_pressure` (byte eviction,
  the over-budget image, both pressure levels).
- `an_image_request_lives_exactly_as_long_as_someone_waits_for_it` (one shared HTTP connection,
  open while a waiter remains, closed by the last drop).
- `bridged_decodes_leave_a_host_runtimes_async_workers_free`,
  `abandoned_loads_never_decode_and_do_not_delay_a_live_one`,
  `decode_slots_go_to_the_earliest_admitted_ready_load` and
  `loads_past_admission_are_bounded_by_count`, in the `image_decode_scheduling` target,
  through real decoder hooks.
- `trim_evicts_image_textures_by_level` (renderer textures by level, between frames).
