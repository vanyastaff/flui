# Application runtime architecture

`flui-app` is the composition root: the platform runners, the loop-scoped
`AppRuntime`, the realm dispatch layer, the raster lane and the platform
wiring. The realm itself (`UiRealm`, its presentations and their frame
transaction) lives in `flui-runtime` (ADR-0083); `crate::app::ui_realm`,
`presentation` and `lifecycle_state` alias its modules for the runners until
the dispatch layer moves there too.

## Invariants

- **The engine stays here.** The realm renders through a
  `flui_runtime::sink::FrameSink` and names no engine type. This crate's two
  sinks are `RasterLane<B>` (the desktop, Android and iOS runners, ADR-0045)
  and `DirectSink` (the web runner). `DirectSink` alone maps `EngineError`s to
  `SubmitVerdict`s for the web runner (pinned by
  `direct_sink_classifies_each_engine_outcome`); the realm's own tests script
  verdicts and never reach it. `raster_lane::RealmRaster` renders a realm's
  draw step over a `DirectSink` for this crate's tests only.
- **A runner's frame is gate → pump → pacing.** Each runner's frame wake is a
  `RealmTask::Pump`: one `UiRealm::enter` holds the owner-inbox drain
  (`UiRealm::drain_owner_inbox`), the pre-frame runner work and the wake gate
  (`wake_action`, `frame_is_dirty`, `FallbackGate`, ADR-0058), which stays per
  backend here; the render arm calls `UiRealm::pump` (ADR-0083 §1), the
  background arm `UiRealm::pump_background`; the pacing after it only reads
  flags. No runner drives scheduler phases itself (pinned by
  `runner_frame_ordering`'s source scan over every runner file, `ios.rs`
  included).
- **Device recovery brackets the pump.** On desktop, Android and iOS,
  `pump_with_device_recovery` runs its pre-frame recovery attempt before the
  pump's begin frame and its post-frame attempt after the post-frame
  callbacks; only `mark_primary_needs_full_repaint` touches the tree, and it
  lands before the pipeline that repaints. The pump's frame timestamp is the
  wake's own `now`. Pinned by the `device_recovery_tests`, among them
  `the_recovery_wrapper_runs_the_whole_frame_transaction`, which fails if the
  wrapper draws without begin or end frame.
- **The raster lane is held for the whole pump.** The lane (the renderer slot
  on web) is the pump's sink, so its lock now spans the transaction, begin
  frame and end frame included, not just the draw step: transient callbacks,
  microtasks, the async poll and post-frame callbacks run under it. That is
  safe because nothing in those phases reaches a lane lock on the owner
  thread synchronously. The other lane lock sites are the frame wake's own
  `try_lock` (which skips a frame rather than wait), the resize hook's
  construction at bootstrap, and the surface-status callbacks on Android and
  iOS, which the platform delivers as their own event, never from inside a
  realm frame; a same-realm dispatch a callback makes is queued, not run
  inline. On web, the renderer slot's other users are the surface applier
  (run from a queued `Resized` dispatch) and the recovery future (spawned,
  so it runs after the frame callback returns). A new lane lock site
  reachable from user code inside a frame must be a `try_lock` or live
  outside the pump.
- **Web runs no frame before its renderer exists.** The web renderer arrives
  asynchronously; until it does, a render wake returns without pumping, so no
  begin, draw or post-frame callback runs, and the realm stays dirty for the
  first animation frame after it arrives. wasm-only: CI's `wasm-check` and
  `wasm-test` compile it; nothing on this host runs it.
- **A window reaches a realm with its bridge.** `runner::presentation_window`
  reads a host window's accessibility bridge once and pairs it with the
  window in a `PresentationWindow` (pinned by
  `a_realm_built_from_a_host_window_publishes_through_its_accessibility`).

## Mapping decisions

### Lifecycle observations are typed and lossless

Desktop primary and secondary windows and UIKit submit their initial execution,
focus and visibility as one `WindowSnapshot` event after registering callbacks.
The queue entry supplies its exact presentation incarnation; the payload cannot
capture a different target. Android host lifecycle callbacks use the existing
`Lifecycle` event. These paths no longer allocate arbitrary realm closures.

Snapshots and lifecycle transitions stay lossless and ordered. A suspended or
unfocused observation can cancel pointer sequences and notify lifecycle listeners;
retaining only the newest observation would erase those effects. This is why
[Tokio watch](https://docs.rs/tokio/latest/tokio/sync/watch/index.html), which retains
only the latest value, is not a replacement for this part of the queue.
Existing `VecDeque` storage and incarnation/close admission remain sufficient;
no new channel or scheduling abstraction is needed for these events.

`queued_window_snapshots_preserve_transitions_and_address_the_sibling` drives
the production FIFO with suspension followed by resumption and observes both on
the addressed sibling. `admitted_close_refuses_a_later_typed_window_snapshot`
pins terminal admission. This preserves the existing lifecycle behavior; it
introduces no new Flutter divergence.

This narrows the arbitrary-operation surface but does not complete ADR-0083's
closed owner vocabulary or bound lossless queue memory. Backend frame pumps still
capture renderer, recovery and pacing state. Their replacement needs
registration-lifetime host drivers and explicit wake admission before extraction
into the runtime; those host resources must not become runtime dependencies.

### Owner work yields between finite batches

The owner-local cross-realm FIFO is cooperative: one logical operation is
never preempted internally, but a continuation callback executes at most 32
operations and then requests one later opportunity. Fresh native roots and
the carried FIFO share that physical-callback budget. Desktop and iOS use the platform
owner signal, Android pokes its window without falsely marking a frame dirty and
acknowledges that opportunity only while native execution is running,
and web consumes the logical continuation on its already-scheduled next RAF.
Stale operations still consume budget because validation and captured-value
destruction are real owner-thread work. A platform adapter may synchronously
re-enter its frame callback (the web window can do this from `request_redraw`);
that entry is a nested root of the existing physical callback and shares its
budget rather than consuming or finishing a second continuation opportunity.

Fresh native roots run synchronously while that callback still has budget;
excess roots join the carried FIFO rather than extending an event-loop turn.
An iOS continuation does not synthesize another background `Pump` for every
retained scene: it spends that callback on the carried FIFO, so a scene count
at or above the batch limit cannot starve old work or grow duplicate pumps on
every continuation. It re-arms one ordinary owner opportunity because the
platform signal coalesces causes; once the carried FIFO drains, that later turn
still services any async/frame wake that shared the continuation callback.
A close therefore installs a terminal barrier for its exact
presentation incarnation when admitted; later work for that address is
refused even before the bounded queue executes the close. The same barrier
revokes authority to install a sibling presentation alongside the closing
address, so a delayed shared-window completion cannot change an admitted
whole-realm close into a partial close. This deliberately
diverges from a single total FIFO across independent native and owner-local
ingress: responsiveness has priority, while per-queue FIFO and terminal
ordering remain explicit and tested.

### Native execution caps remain presentation-local

ADR-0072 adds a window execution observation to the existing presentation facts.
Suspension caps only that presentation at Paused; a running sibling can keep the
shared scheduler eligible. Host suspension and terminal close remain stronger than
late local Running/focus/visibility events. Callback registration precedes a batch
snapshot, whose execution/focus/visibility fields are committed together before
reconciliation and public lifecycle notification. Input cancellation uses the same
addressed path as focus loss and runs before lifecycle observers.

The facade continues to expose AppLifecycleState through its existing lifecycle
handle, without adding raw platform control types. A surface restoration failure
stays released and skips GPU work. Existing device recovery does not imply an
automatic surface recreation retry; another availability request is currently
required. Scene migration and background owner waking remain explicit follow-ups.

### UIKit process and session ownership

The owner-only background turn is a typed `BackgroundPump`, not a captured
callback: drain the addressed realm's owner inbox, then poll its async driver
without a frame. Poll-generated commands remain for the next owner opportunity.
It uses the existing exact-address close fence and finite FIFO budget; nested
wakes enqueue rather than recursively poll. Background operations remain lossless
and are not coalesced across other operations. This adds no public scheduling
contract or driver registry; renderer-capturing frame callbacks still require
registration-owned drivers before the owner host can move into the runtime.
The dispatcher tests exercise inbox ordering, async polling and reentrant close
admission through this same production operation.

The UIKit runner starts services, execution pools and its development watcher
once per process. Its private session controller installs a real realm only for
a fresh scene session; reconnect selects the retained realm. Terminal discard
uses the existing addressed close path and removes the session before disposal.
The controller's generic key permits the same production ownership logic to run
with real headless realms in host tests; it is not another lifecycle reducer or a
public raw-platform capability. Root configurations can retain application-owned
state while a new session creates fresh `ViewState`. See ADR-0073 for native
attachment lifetime, panic containment and platform limits.
