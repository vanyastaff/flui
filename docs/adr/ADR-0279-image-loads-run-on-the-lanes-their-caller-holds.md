# ADR-0279: Image loads run on the lanes their caller holds

- **Status:** Accepted (2026-10-31)
- **Date:** 2026-10-31
- **Supersedes:** [ADR-0105](ADR-0105-asset-validation-and-bridge-progress.md) "Decision", its
  second and third paragraphs only (the bridge's runtime selection and its owned runtime);
  [ADR-0250](ADR-0250-asset-registry-keeps-decoded-images.md) §2 only for how a load is aborted
  and what a dropped load does to its waiters, and §3 only for where a decode runs.
- **Closes:** [ADR-0047](ADR-0047-unified-execution-services.md)'s named residual, the asset
  registry's own runtime.
- **Related:** [ADR-0272](ADR-0272-widgets-run-background-work-through-a-capability.md) (the
  background capability whose `Executor` the loads run on)

## Context and problem statement

`AssetRegistry` read and decoded images on a Tokio runtime it chose itself: an injected handle,
else an ambient multi-thread runtime, else one it started. That runtime sat outside the UI
runtime's admission windows and shutdown (ADR-0047), and the thread it started was the last one
FLUI kept outside them. ADR-0272 gives widgets the UI runtime's lanes as a capability, but
`flui-assets` sits below `flui-view` and cannot reach the capability, and its network loader's
HTTP client needs a Tokio reactor wherever its request is polled. The question is how a registry
below the view layer runs its loads on the lanes of whoever asks, while a load stays shared by
every request for a source.

## Decision drivers

- No thread or runtime is started outside the UI runtime's lanes (ADR-0047).
- `flui-assets` depends down the tiers only: `flui-platform` (tier C) is below it, `flui-view`
  is not.
- A load is shared: one request leaving must not cancel it for the others, and no request is left
  waiting on a load nobody runs.
- The HTTP client's need for a reactor fails loudly and typed, never as a panic.

## Considered options

1. The caller hands its lanes (`Arc<dyn Executor>`) to each load; the registry keeps owning the
   load.
2. The registry returns the work (a read future and a decode step) and the caller runs it.
3. Keep the registry's runtime and inject the host's `Handle`.

## Decision

Chosen option: 1, because the registry keeps ownership of a shared load, so who started it no
longer decides how long it lives.

1. **Lanes per load.** `AssetRegistry::load_image(path, lanes)` and
   `load_network_image(url, lanes)` take `lanes: Option<Arc<dyn flui_platform::executor::Executor>>`
   and return `ImageLoad`. The first request for a source spawns the load on its lanes' IO lane
   (`spawn_io`); the read finishes by asking for a decode slot, and the decode runs on the compute
   lane (`spawn_compute`) behind the slots of ADR-0250 §3. `BridgeRuntime`,
   `AssetRegistryBuilder::with_runtime_handle` and the `rt-multi-thread` dependency are gone.
   `flui_view::background::Background::executor` gives a widget's lanes to the registry;
   `ImageProvider::resolve_async` receives the `Background` the `Image` widget holds.
2. **The registry owns the load.** A spawned load is cancelled by a signal the pending entry holds:
   the last subscriber's drop removes the entry and the signal fires, and the load sees it before
   its next step, so a load cancelled before its bytes are ready never asks for a decode slot. A
   subscriber that is not the last leaves the load running.
3. **Where no lanes run a load, the request does.** With no lanes, or when `spawn_io` refuses with
   `Saturated` or `Unavailable`, the load runs inside the requesting `ImageLoad`, on the thread
   that polls it; a refused `spawn_compute` decodes where the load runs. `ShuttingDown` answers
   `AssetError::LanesRefused`.
4. **A dropped load is handed over.** A load that ends without an outcome (the request it ran in
   left, or its lanes shut down and dropped it) closes its waiters; each admits itself again and
   the first becomes the lead on its own lanes. A load that panics answers its waiters with the
   failure instead (its drop guard sees `std::thread::panicking`), so a failing load is not rerun.
   One `ImageLoad` takes over at most four closed loads before it answers with an error.
5. **The reactor is checked, not assumed.** A network load checks
   `tokio::runtime::Handle::try_current` before its request and answers
   `AssetError::ReactorUnavailable` without one. A native host's default IO lane runs inside a
   Tokio runtime; the headless host's deterministic lanes, a host executor and a load with no
   lanes may not.

### Consequences

- Good, because every image read and decode runs inside the UI runtime's admission and shutdown,
  and `flui-assets` starts no thread.
- Good, because a request leaving, its lanes retiring or a host lacking lanes never strands the
  other requests for a source.
- Bad, because a network image needs a Tokio reactor on the IO lane it runs on: on the headless
  host it is `ReactorUnavailable` (`docs/gaps.md`), until a platform HTTP capability replaces the
  Tokio-bound client.
- Bad, because file reads are blocking reads on the IO lane, in 256 KiB chunks with a yield
  between them.
- Neutral, because a load with no lanes, or refused for room, reads and decodes on the thread that
  polls it: the owner thread, for the `Image` widget.

### Confirmation

- `image_loads_run_on_their_lanes_and_survive_the_requests_that_leave` (`flui-assets`): lanes,
  no lanes, saturated lanes, a request leaving, the running request leaving, lanes shutting down
  with a request from other lanes waiting, and a load cancelled before its read never reaching the
  decoder.
- `network_loads_reuse_connections_and_recover_after_decode_errors`, row "no reactor", and
  `an_image_request_lives_exactly_as_long_as_someone_waits_for_it`.
- `decodes_leave_the_io_lane_free` and the other `image_decode_scheduling` tests, on a native
  host's default lanes.
- `network_image_loads_on_a_native_hosts_default_lanes` (`flui-widgets`): `Image::network` on
  `ExecutionServices::with_defaults`.
- `cargo xtask workspace` holds the `flui-assets` → `flui-platform` edge to the tier order.

## Pros and cons of the options

### 1. Lanes per load

- Good, because the registry's sharing, slots and cancellation stay in one place.
- Good, because `flui-assets` names only the `Executor` contract, not `flui-view`.
- Bad, because the registry's own futures must handle refusal and handover themselves.

### 2. The caller runs the work

- Good, because the registry needs no executor at all.
- Bad, because the work lives in the request that started it: when that request leaves, the
  others wait on work nobody runs.

### 3. Inject the host's runtime handle

- Good, because it changes the least.
- Bad, because it freezes Tokio into the embedding contract, which ADR-0047 rejects, and keeps a
  second admission path beside the lanes.

## More information

Revisit when a platform HTTP capability lands: the network load then needs no reactor, and
`ReactorUnavailable` goes away.
