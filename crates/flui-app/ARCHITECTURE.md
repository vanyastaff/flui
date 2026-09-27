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
  wake's own `now`. Pinned by the `device_recovery_tests`.
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

The UIKit runner starts services, execution pools and its development watcher
once per process. Its private session controller installs a real realm only for
a fresh scene session; reconnect selects the retained realm. Terminal discard
uses the existing addressed close path and removes the session before disposal.
The controller's generic key permits the same production ownership logic to run
with real headless realms in host tests; it is not another lifecycle reducer or a
public raw-platform capability. Root configurations can retain application-owned
state while a new session creates fresh `ViewState`. See ADR-0073 for native
attachment lifetime, panic containment and platform limits.
