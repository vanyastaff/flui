# flui_assets architecture

## Scope

`flui-assets` is an application-owned decoded-image registry. It has no generic
asset trait, type-erased cache map, handles, font loader or process-global
cache. ADR-0317 is the boundary decision; ADR-0250, ADR-0279 and ADR-0280 retain
the decoded-image retention, execution and HTTP rules.

```text
Image widget or direct caller
        │ Arc<AssetRegistry> + ImageSource + LoadServices
        ▼
AssetRegistry
        │ cached_image: byte-budgeted LRU probe
        │ load_image / load_network_image: shared pending load
        ▼
IO lane or polling thread → encoded bytes → compute lane or polling thread
        │ bounded RGBA8 decode
        ▼
Completed Image + registry retention (when within budget)
```

## Modules

- `registry::images` owns source identity, completed images, pending loads,
  subscriber notifications, file/HTTP source work and load scheduling.
- `registry::ordered_slots` owns admission and decode reservations.
- `registry::decode` performs header/allocation-bounded RGBA8 decoding.
- `registry` exposes `AssetRegistry`, its builder and memory reclamation.
- `error` exposes the typed source, HTTP, decode and lane failures.

The `images` feature gates image loading and its `image`/`lru` dependencies.
Network loading uses the same feature and an injected HTTP client. The crate's
wasm support metadata remains unchanged; this design does not claim a new wasm
implementation.

## Registry and source identity

A registry owns one `DecodedImages`. There is no global registry and no second
typed completed-value cache. Two registries never share entries implicitly.

`ImageSource::Asset` owns a shared path string. A network source combines its
URL with the HTTP client's authority, so equal URLs under different clients do
not share loads or pixels. `cached_image` is synchronous and only refreshes an
existing completed entry's recency.

A pending entry also has a ticket allocation. Completion compares that ticket
with the current entry, preventing a cancelled or replaced generation from
publishing into a newer one.

## Retention

Completed images are kept in an LRU weighed by `width * height * 4`. The budget
defaults to 256 MiB, and 64 MiB on Android, iOS and the web. Images over the
budget are returned but not inserted; zero disables completed retention.
Errors are never inserted.

Moderate `ReclaimMemory` pressure trims to half the budget and critical pressure
clears it. Returned images remain owned by their callers. In-flight loads are
not cancelled by pressure.

## Shared load lifetime

Each caller receives an `ImageLoad` subscriber. The first cold caller creates a
pending entry; later callers join it. The entry and its work remain while any
subscriber exists.

Dropping the last subscriber removes the entry, cancels the ticket and sends its
cancel signal after releasing internal locks. HTTP is dropped immediately. A
file read stops at its next 256 KiB chunk. A load cancelled before bytes are
ready never requests decode capacity, and a queued decode that has not started
never runs.

If a lane or polling request drops the run without an outcome, waiters are
closed and may admit the load again. The first becomes leader on its own
services. A decoder panic becomes `DecodeFailed` for that load instead of
unwinding through the lane. Failures are not retained.

## Execution services

`LoadServices` contains optional caller lanes and HTTP. The registry starts no
runtime, reactor or thread.

- File reads and HTTP requests run on IO.
- RGBA8 decoding runs on compute.
- Without lanes, or when admission is refused for lack of room, a subscriber
  drives work where it polls.
- Lanes shutting down produce `LanesRefused` for a refused lead; remaining
  waiters on healthy services can take over unfinished work.
- HTTP absence produces `HttpUnavailable`; transport and non-2xx results are
  `Http` and `HttpStatus`.

The host's HTTP capability owns concrete networking, TLS, pooling and timeouts.
`flui-assets` carries no HTTP implementation or `network` feature.

## Admission and decode scheduling

Each decode thread provides four load slots. A lane-bound load reserves a slot
when requested; a no-lane load reserves on first poll, so an unpolled future
neither blocks others nor creates an undriven pending entry. A load holds its
slot through reading and decoding.

A queued load starts on IO only after receiving its slot. If the lane then has
no room, the run is parked and the first polled subscriber drives it locally.
Slot handoff is iterative, including long shutdown queues, rather than nested
through each refused load.

Load slots are granted in admission order. Decode slots use the earliest
admitted load whose bytes are ready. A fetching load holds no decode slot, so
ready work is not blocked behind it. Waiting for compute room uses the lane's
room notification rather than occupying IO.

## Bounds

- Only regular files are opened.
- Encoded files and HTTP bodies are capped at 256 MiB.
- File reads use 256 KiB chunks with cancellation checks.
- The image header's `width * height * 4` uses checked arithmetic.
- Decoder output and allocation are capped at 256 MiB, or 128 MiB on Android,
  iOS and the web.
- Concurrent decodes default to half the cores, at most four, and are explicitly
  configurable.

These bounds are independent: the completed-image LRU does not account for
in-flight encoded bytes or decoder scratch memory.

## Synchronization and user code

One internal mutex coordinates the completed cache and pending map. Short
critical sections commit state first. Wakers, completion sends, cancel signals,
lane submission and evicted pixel destruction run after releasing it. Lane
submission can drop a refused job reentrantly; that drop follows the same
committed-state path.

No build/layout/paint operation waits on async IO. Widgets use the synchronous
probe and subscribe outside that probe when needed.

## Behavioral evidence

- `decoded_images_stay_within_their_byte_budget_and_give_back_under_pressure`:
  byte eviction, over-budget delivery and both pressure levels.
- `image_loads_run_on_their_lanes_and_survive_the_requests_that_leave`: lanes,
  no-lane polling, sharing, cancellation, handover and recovery.
- `network_loads_fetch_with_the_callers_client_and_recover_after_errors` and
  `one_registry_keeps_two_clients_images_of_one_url_apart`: HTTP capability,
  retry and source identity.
- `decodes_leave_the_io_lane_free`,
  `abandoned_loads_never_decode_and_do_not_delay_a_live_one`,
  `decode_slots_go_to_the_earliest_admitted_ready_load` and
  `loads_past_admission_are_bounded_by_count`: real decoder-hook scheduling.
- `decoded_pixels_are_bounded_before_decoding`: malicious header dimensions.
- `assets_feature_exposes_and_loads_images_through_the_facade`: an external
  consumer mounts a real asset image through `flui::assets` and verifies its
  decoded pixels and normal dependency graph.

## References

- [ADR-0317](../../../docs/adr/ADR-0317-the-asset-registry-is-image-only-and-the-facade-opts-into-it.md)
- [ADR-0250](../../../docs/adr/ADR-0250-asset-registry-keeps-decoded-images.md)
- [ADR-0279](../../../docs/adr/ADR-0279-image-loads-run-on-the-lanes-their-caller-holds.md)
- [ADR-0280](../../../docs/adr/ADR-0280-http-is-a-capability-of-the-host.md)
