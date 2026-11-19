# flui-assets architecture

`flui-assets` is the decoded-image cache of FLUI
([ADR-0385](../../../docs/adr/ADR-0385-flui-assets-is-the-decoded-image-cache.md)).
`flui-widgets` resolves `Image::asset` and `Image::network` through an `ImageCache`; nothing
else in the framework loads through it, and it holds no generic asset type. Its public surface
is the crate root:

| Item | What it is |
|------|------------|
| `ImageCache` | The decoded images one owner keeps, and the loads that fill them; `ReclaimMemory` gives them back under pressure. |
| `ImageCacheConfig` | The byte budget and the number of decodes at once. |
| `ImageSource` | What a load reads: a file path, or a URL with the authority of the client that fetches it. |
| `LoadServices` | The lanes and the HTTP client a load runs on, both the caller's. |
| `ImageLoad` | The future one request awaits; dropping the last one cancels the load. |
| `AssetError` | Why a load failed. |

The crate has three private modules: `error`, `image_cache` (the public types) and
`image_cache::loads` (the LRU, the pending map, the load and decode slots and the load's run),
with `image_cache::ordered_slots` beneath it.

## Network images

A network image is fetched with the HTTP client its caller passes in
`LoadServices` (the host's `HttpCapability`, ADR-0280); this crate links no
HTTP stack and keeps no connection pool. The client drives the request, so the
lane a load runs on needs no async runtime, and a load with no lanes fetches
where it is polled. A load without a client answers `AssetError::HttpUnavailable`,
a failed request `AssetError::Http` and a status other than 2xx
`AssetError::HttpStatus`; none is kept, so a later request retries. Each request
limits its body to 256 MiB. `network_loads_fetch_with_the_callers_client_and_recover_after_errors`
covers recovery after invalid image bytes, no lanes, an error status, a failed
request, no client and a load handed over to the next request's client.

## Decoded images are kept, shared and cancelled by their cache

Each cache keeps the decoded images of its loads (`load_image`,
`load_network_image`) in its own LRU, bounded by pixel bytes
(`ImageCacheConfig::budget`; 256 MiB, 64 MiB on Android, iOS and the web); an
image larger than the budget is handed to its waiters and not kept.
`cached_image` is the synchronous probe a frame makes before it starts a load.
There is no process-wide image cache, so equal sources in two caches are two
images (ADR-0250). A network source is its URL with the `HttpAuthority` of the
client that fetches it, so two clients answered different images at one URL
never share a decoded image or a load (ADR-0280 §6); a load without a client
shares nothing with one that has one.

A load runs on the `LoadServices` its first request hands it: lanes (an
`Arc<dyn flui_platform::executor::Executor>`, the UI runtime's for the `Image`
widget) and an HTTP client (`flui_platform::http::Http`, the host's for that
widget, ADR-0280): read or fetched on the IO lane, decoded on the compute lane.
The cache starts no runtime or thread (ADR-0279). With no lanes, or lanes
refusing it for want of room, the load runs inside the requesting `ImageLoad`,
on the thread that polls it; lanes shutting down answer
`AssetError::LanesRefused`.

A request for a source already loading joins that load. The load is the
cache's: every `ImageLoad` waiting on it is a subscriber, and only when the
last one is dropped is the pending entry removed, its ticket cancelled and its
cancel signal sent, which the load sees before its next step. An HTTP request is
dropped at once. A file is read in 256 KiB chunks with a yield between them, so
the read stops at the next chunk. A load cancelled before its bytes are ready
never asks for a decode slot, and a decode that has not started never runs. The
ticket's allocation is the load's identity: a load finishing after its entry was
abandoned or replaced compares tickets and leaves the newer entry alone.

A load that ends without an outcome, because the request it ran in left or its
lanes shut down and dropped it, closes its waiters, and each admits itself again:
the first becomes the new lead, on its own lanes. A load that panics answers its
waiters with the failure instead, from a drop guard that sees the unwind.

Decodes run at most `ImageCacheConfig::decode_threads` at once (half the cores, at most four),
never on the IO lane. A compute lane with no room is waited for through
`Executor::room`, and the decode spawned again once it has room; a compute
lane whose workers could not start hands the decode back to where the load runs;
lanes shutting down, by refusing the decode or dropping it unrun,
close the load unsettled for a waiter on other lanes to take over. At admission,
on the calling thread and under the lock that assigns its order, a load reserves
one of four load slots per decode thread. A load bound for lanes is admitted when
it is requested; a load with no lanes runs only while its request polls it, so it
is admitted at its first poll: loads a caller holds unpolled keep no slot from
the one it awaits, and open no entry that a request for the same source would
wait on with nobody driving it. A load bound for lanes is spawned on their IO
lane only once it holds its slot: a queued one is a start the slot queue calls,
on the thread that freed the slot, so loads waiting for a slot spend none of
the IO lane's admission window and other IO work is never refused for them. If
the lanes have no room by then, the run is parked and every subscriber told; the
first one polled claims it and drives it where it is polled; if they are shutting down, the lead
answers `AssetError::LanesRefused` and the rest take the load over. A load
waits for its slot before it reads and holds it until its decode
returns, so the reads in flight and the encoded bytes waiting to decode are
bounded by count, and each by size: a response body by `MAX_BODY_BYTES`, a file
by the same 256 MiB (`an_oversized_file_is_refused_before_it_is_read`), refused
before more than that is buffered. Only a regular file is opened, so a FIFO or a
device cannot block the shared IO lane in `open`. Once its bytes are read it asks for a decode
slot. Load slots are granted in admission order. A decode slot goes to the
earliest-admitted load whose bytes are ready; a load still fetching holds none,
so a ready load admitted later decodes first rather than leaving the decoder idle
behind a slow response. Strict admission order would be head-of-line blocking:
one stalled request would stop every visible image behind it. A load abandoned
while it waited for either slot skips the decode and frees its load slot.

One mutex guards the cache and the pending map. Senders are completed, cancel
signals sent, loads spawned and evicted pixels freed after it is released: lanes
that refuse a load drop it inside `spawn_io`, and that drop settles its entry
through the same lock.

Under memory pressure the cache trims decoded images to half the budget
(moderate) or drops them all (critical); loads in flight are left alone.

`decoded_images_stay_within_their_byte_budget_and_give_back_under_pressure`
pins byte eviction, the over-budget image and both pressure levels.
`image_loads_run_on_their_lanes_and_survive_the_requests_that_leave` pins the
lanes, sharing, handover and cancellation.
`an_image_request_lives_exactly_as_long_as_someone_waits_for_it` pins coalescing
onto one HTTP connection, which stays open while one waiter remains and closes
when the last is dropped. The separate `image_decode_scheduling` target, which
registers real decoder hooks, runs
`decodes_leave_the_io_lane_free` (a decode waits for a task on the IO lane's only
worker) and
`abandoned_loads_never_decode_and_do_not_delay_a_live_one` (one decode thread:
five loads abandoned behind a running decode never decode, and the live load
queued after them decodes next) and
`decode_slots_go_to_the_earliest_admitted_ready_load` (with the decoder free,
a file load decodes while an earlier network load still fetches; with both
waiting behind a running decode, the earlier-admitted network load goes first)
and `loads_past_admission_are_bounded_by_count` (one decode thread: while a
decode holds its slot only three more requests go out, and the next starts
when it returns).
`a_load_waiting_for_its_load_slot_holds_no_place_on_the_io_lane` (one decode
thread: six held loads put four tasks on the IO lane, a freed slot spawns the
next; lanes with no room park the run for the subscriber polled first, even
when the lead is never polled again; lanes shutting down refuse only the lead,
and a long queue of them is refused in turn on a small stack).

A network image's request carries `HttpRequest`'s default deadline (two minutes,
body included), so a server that drips one byte at a time cannot hold a load
slot forever, and its 256 MiB body limit; the host's built-in client also gives
up on a connection and on a silent response after 30 seconds.

Every image decode reads the header first and refuses an
image whose RGBA8 pixels would pass 256 MiB (128 MiB on Android, iOS and the
web), which is also the decoder's allocation limit: an image that compresses
small but decodes huge fails with a typed error instead of allocating. With at
most four decodes at once, their transient memory stays a few times that.
`decoded_pixels_are_bounded_before_decoding` decodes a header that declares
too many pixels.
