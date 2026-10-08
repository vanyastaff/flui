# Application runtime architecture

`flui-app` is the composition root: the platform runners, the loop-scoped
`AppRuntime`, the UI runtime dispatch layer, the raster lane and the platform
wiring. The UI runtime itself (`UiRuntime`, its presentations and their frame
transaction) lives in `flui-runtime` (ADR-0083); `crate::app::ui_runtime`,
`presentation` and `lifecycle_state` alias its modules for the runners until
the dispatch layer moves there too.

## Invariants

- **Native owner replacement establishes the logical host before construction.**
  The shared runtime builder seeds accepted preferences before mounting a root;
  Android/web preparation retains that host through publication.
  `bootstrap_keeps_the_host_that_seeded_the_first_build` exercises the builder,
  first rendered consumer and publication. `a_replacement_platform_starts_a_new_preference_owner`
  verifies that the old source closes and its values do not enter the next build.
  Outgoing hooks, clipboard and owner resources retire after the replacement is
  visible and outside the composition-root borrow. `platform_replacement_contains_reentrant_retirement`
  covers reentrant updates, single/competing destructor failures and the usable
  replacement after containment. Installation samples the native owner outside
  the composition-root borrow, checking both native and logical owner identity
  before accepting its result. Obsolete values and errors are discarded.
  `obsolete_native_observation_cannot_update_a_replacement_host` injects replacement
  during a getter; `windows_bootstrap_accepts_preferences_before_the_first_window`
  runs the real native source and observes text scale in the first build without
  changing OS settings. Read failure leaves observations unknown and does not
  skip outgoing resource retirement. Desktop owner wakes refresh through the
  same identity-fenced path before window completions; source caches avoid native
  reads when no invalidation is pending. `owner_wake_refreshes_installed_preference_consumers`
  drives the registered headless owner hook and verifies the retained widget's
  next rendered scale. This is delivery evidence, not an actual OS settings change.
  Windows retries native read failures through its window-independent loop wait;
  deferred reads retain the accepted snapshot without another failure diagnostic.
  `deferred_native_reads_preserve_accepted_preferences` checks the next build and
  subsequent successful refresh through the app builder.
  Mobile live refresh and end-to-end OS-generated change acceptance remain pending.

- **Admission is not window readiness.** A desktop rendered installation publishes its
  native driver and logical membership together, then applies initial facts
  through the runtime owner before reporting success. The main-window request
  waits for that receipt; secondary completion retains its loop-liveness
  reservation through initialization. `main_window_waits_for_deferred_runtime_publication`
  and `secondary_installation_contract` cover delayed publication, early native
  closure and initialization failure. Refused or failed secondary installation
  releases its reservation and leaves no registered presentation.
  Dropping a pending installation receipt cancels publication independently of
  native close delivery. Session installation retains its native acknowledgement
  until the receipt succeeds; a reconnect replaces that acknowledgement without
  rebuilding the logical runtime. Only ready sessions receive background work.
  `session_installation_contract` exercises these rules through the headless host,
  including abandonment without a native close callback. It does not establish
  UIKit attachment or simulator behavior.
  Unpublished native drivers and close handlers retire independently on
  abandonment or deferred cancellation. Competing destructor failures preserve
  the first failure; an existing unwind stays authoritative.
  `unpublished_window_retires_native_resources_independently` covers these paths
  and subsequent frame delivery. Containment does not rescue multiple panicking
  destructors within one opaque driver or handler aggregate.
  Native window closure is owned throughout preparation, pending publication and
  initialization. Only successful initialization releases that obligation;
  dropping an unfinished installation closes its retained window, even when a
  caller still holds another native handle.
  `unfinished_installation_closes_its_native_window` covers pre-submit abandonment
  and deferred receipt cancellation through real headless close callbacks.
  Development-agent notification belongs to the installation, not the frame
  driver. Native initialization may synchronously close the window or tear down
  the host; joint membership and native closure are rechecked before handing the
  agent its window and again after the hook before reporting readiness.
  `agent_notification_follows_live_native_initialization` covers native close,
  teardown, initialization failure, closure from the hook and subsequent frame
  delivery through the private native-initialization seam. It does not establish
  operating-system activation behavior.

- **Quit is a closed operation.** The host stops installed runtimes in mount
  order and restores each checkout before continuing. A failing lifecycle
  listener cannot skip a sibling; reentrant work waits until notifications
  finish. `panicking_stop_notifies_siblings_and_restores_runtime_delivery`
  covers individual and competing listener failures and subsequent delivery.

- **Recovery does not start new windows.** Owner completion still retires native
  resources and settles terminal notifications after a failure, but leaves
  new main/secondary installation work for a healthy owner opportunity.
  An already-published secondary installation still settles its receipt and
  releases its loop-liveness reservation during recovery.
  `owner_failure_defers_new_window_work_until_recovery` asserts the original
  failure, no premature installer call, and delivery of the retained request.

- **Native roots preserve accepted event order.** A fresh input or window event
  joins the existing owner FIFO, including when a continuation has been posted
  or failed. A physical callback spends its shared budget on that FIFO; it cannot
  prioritize a new key over older metrics, lifecycle or input. A keyboard reply
  unavailable before the native callback returns suppresses native default
  handling. `native_keyboard_cannot_overtake_queued_window_changes_and_keys`
  exercises the installed input callback and surface-metrics application;
  `carried_work_shares_one_callback_budget_across_runtimes` pins bounded progress.
- **Prepare windows before publishing membership.** Native identity reads and
  presentation assembly run outside `APP_RUNTIME` borrows. Assembly can reenter
  the host, so shared installation revalidates the exact authorizing presentation
  before publishing either membership. Refused presentations remain owned outside
  the registry borrow. `native_identity_is_observed_before_registry_publication`
  and `presentation_assembly_reentry_revalidates_its_authorizer` exercise platform
  callbacks, authorizer closure with and without a surviving sibling, and reuse
  of the refused window's native identity by the next installation.
- **The engine stays here.** The UI runtime renders through a
  `flui_runtime::sink::FrameSink` and names no engine type. This crate's two
  sinks are `RasterLane<B>` (the desktop, Android and iOS runners, ADR-0045)
  and `DirectSink` (the web runner). `DirectSink` alone maps `EngineError`s to
  `SubmitVerdict`s for the web runner; the UI runtime's own tests script verdicts
  and never reach it.
- **Resize uses the installed driver's lifetime.** Addressed resize checks out
  the same native driver as frame delivery. Its strong lease restores the exact
  registration or retires it after reentrant teardown, without consulting TLS.
  A sibling presentation cannot resize that driver's surface. Desktop, Android
  and iOS clone the raster resize hook under the lane guard and apply it after
  releasing the guard; web preserves canvas rounding at fractional DPR.
  `resize_uses_the_installed_driver_lease_through_failure_and_teardown` pins
  retry, reentrant teardown, competing resource failure and subsequent delivery.
- **Native operations retain their owning registry.** `NativeBindings` owns window
  routes, frame drivers and close-request registrations. Frame and resize hold it
  through lease return, even when callbacks release the last external handle.
  Final release withdraws routes and retires resources independently, preserving
  the first failure. An outliving close-request router retains no old handlers.
  `native_operation_retains_owners_after_the_last_external_handle_is_released`
  covers normal release and competing operation, driver and handler failures.
- **Transient render failure retains demand.** Both sinks distinguish `Retry`
  from terminal `Failed` and device recovery. The raster completion publishes
  retry debt reliably even when telemetry acks are full; the UI runtime retains epochs
  and requests a paced full repaint (ADR-0101). Pinned by
  `transient_and_hard_failures_map_consistently_in_lane_and_direct_sink`.
- **A runner's frame is gate → pump → pacing.** Each runner's frame wake carries
  `RuntimeTask::Frame` with its installed surface binding. The concrete driver
  owns its backend resources across wakes; one `UiRuntime::enter` holds the owner-inbox drain
  (`UiRuntime::drain_owner_inbox`), the pre-frame runner work and the wake gate
  (`wake_action`, `frame_is_dirty`, `FallbackGate`, ADR-0058), which stays per
  backend here; the render arm calls `UiRuntime::pump` (ADR-0083 §1), the
  background arm `UiRuntime::pump_background`; the pacing after it only reads
  flags. No runner drives scheduler phases itself (pinned by
  `runner_frame_ordering`'s source scan over every runner file, `ios.rs`
  included). A driver lease restores its resources without consulting TLS;
  closing its originating presentation retires them even if another presentation
  becomes the logical primary. Close admission also refuses later async renderer
  publication, while an earlier accepted frame can still finish.
  `installed_frame_driver_contract` exercises product scene submission, stale
  binding refusal, reentrant teardown, competing failures and close-time
  publication through a private driver seam without requiring a native GPU.
  Android and web register native close and platform quit through
  `WindowInstall::terminal_callbacks`; the same table invokes both callbacks
  through the headless platform and refuses publication after either terminal
  notification. This tests callback wiring, not native GPU destruction or browser
  execution.
- **Native retirement follows registry mutation.** The close-request router
  returns removed handlers without invoking their destructors. App completion
  releases handlers and frame drivers individually through `NativeRetirement`,
  outside TLS and store borrows, preserving the first failure.
  `native_owners_retire_independently_after_registry_removal` covers ordinary
  close and full teardown, including competing destructor failures and the next
  scene. `retiring_a_handler_preserves_its_reentrant_registration` pins that a
  handler installed during an outgoing capture's destruction remains registered.
- **Owner events describe observations, not executable callbacks.**
  `RuntimeTask::Event(RuntimeEvent)` carries presentation input and window state.
  `RuntimeDispatcher` carries fonts, host lifecycle and background work by
  `UiRuntimeId`; closing its original window does not revoke runtime authority.
  Both use one FIFO and physical callback budget. `FontsChanged` invalidates all
  presentations; `PrimarySurfaceRestored`
  requests a full repaint of the primary, which owns the current UI runtime sink.
  Native surface callbacks release their raster-lane guard before dispatch.
  `TestCallback` exists only under `cfg(test)` for private failure injection.
  The installed driver executes its backend frame protocol; extracting the owner
  host remains ADR-0083's migration work.
  `owner_dispatch_matrix` pins font registration fan-out/reentry and
  `recovered_surface_notification_resubmits_the_scene` pins a real scene
  submission after an idle frame, rather than a dirty-flag change.
  `font_notification_survives_the_primary_window_closing_before_delivery`
  separates font invalidation from redraw caused by close. Background polling
  also survives primary close and rejects a replaced runtime incarnation.
- **Device recovery brackets the pump.** On desktop, Android and iOS,
  `pump_with_device_recovery` runs its pre-frame recovery attempt before the
  pump's begin frame and its post-frame attempt after the post-frame
  callbacks; only `mark_primary_needs_full_repaint` touches the tree, and it
  lands before the pipeline that repaints. The pump's frame timestamp is the
  wake's own `now`.
- **The raster lane is held for the whole pump.** The lane (the renderer slot
  on web) is the pump's sink, so its lock now spans the transaction, begin
  frame and end frame included, not just the draw step: transient callbacks,
  microtasks, the async poll and post-frame callbacks run under it. That is
  safe because nothing in those phases reaches a lane lock on the owner
  thread synchronously. The other lane lock sites are the frame wake's own
  `try_lock` (which skips a frame rather than wait), the addressed resize driver's
  short acquisition of its resize hook, and the surface-status callbacks on Android and
  iOS, which the platform delivers as their own event, never from inside a
  UI runtime frame; a same-UI runtime dispatch a callback makes is queued, not run
  inline. On web, the renderer slot's other users are the driver's resize operation
  (run from a queued `Resized` dispatch) and the recovery future (spawned,
  so it runs after the frame callback returns). A new lane lock site
  reachable from user code inside a frame must be a `try_lock` or live
  outside the pump.
- **Web runs no frame before its renderer exists.** The web renderer arrives
  asynchronously; until it does, a render wake returns without pumping, so no
  begin, draw or post-frame callback runs, and the UI runtime stays dirty for the
  first animation frame after it arrives. wasm-only: CI's `wasm-check` and
  `wasm-test` compile it; nothing on this host runs it.
- **A window reaches a UI runtime with its bridge.** `runner::presentation_window`
  reads a host window's accessibility bridge once and pairs it with the
  window in a `PresentationWindow`.
- **The app's fonts are one host scan and one collection, fed off the owner
  thread.** `SharedEngineServices::resolve`, reached before the first UI runtime is
  built, builds the app's `FontCollection` with the bundled faces and a
  `HostFontFeed` (`FontCollection::with_host_feed`); once the fonts the app
  registered before the start are in, the runtime launches the feed on a
  `flui-host-fonts` thread, which scans the host once (`HostFonts::scan`,
  fontdb), adds its faces and wakes the owner. The first frame does not wait
  for it. Every top-level owner turn (`dispatch_platform_ui_runtime`) asks the
  runtime whether the collection's generation moved since the UI runtimes were
  last told (`AppRuntime::take_font_change`) and, if so, sends every UI runtime
  `UiRuntime::fonts_changed`; a registration is announced the same way
  (`runner::fonts::announce_font_change`). A notice a UI runtime refuses because
  its presentation is closing or stale is not retried: its pipelines still
  see the new generation at their next frame. The feed's thread is detached
  and outside the runtime's execution services, which may refuse a job and
  drop it, while the feed must run once (ADR-0092 §7). The scan is a value dropped once
  the feed is done, and no font state is process-global. Every UI runtime the
  runners build gets a clone of that one collection. This crate's unit tests
  park the feed with its wake (`park_host_feed`), so none lands in the middle
  of a test, and a test runs both
  (`the_runtime_launches_one_host_feed_for_every_ui_runtime`,
  `the_host_feed_runs_off_the_owner_thread_and_wakes_once`,
  `a_landed_host_feed_wakes_every_ui_runtime_window`).

## Mapping decisions

### Lifecycle observations are typed and lossless

Desktop primary and secondary windows and UIKit submit their initial execution,
focus and visibility as one `WindowSnapshot` event after registering callbacks.
The queue entry supplies its exact presentation incarnation; the payload cannot
capture a different target. Android host lifecycle callbacks use runtime-scoped
`RuntimeDispatcher::lifecycle`. These paths carry closed operations.

Snapshots and lifecycle transitions stay lossless and ordered. A suspended or
unfocused observation can cancel pointer sequences and notify lifecycle listeners;
retaining only the newest observation would erase those effects. This is why
[Tokio watch](https://docs.rs/tokio/latest/tokio/sync/watch/index.html), which retains
only the latest value, is not a replacement for this part of the queue.
Existing `VecDeque` storage and incarnation/close admission remain sufficient;
no new channel or scheduling abstraction is needed for these events.

**Unasserted:** no test pins this. This preserves the existing lifecycle
behavior.

This narrows the arbitrary-operation surface but does not complete ADR-0083's
closed owner vocabulary or bound lossless queue memory. Backend frame pumps still
capture renderer, recovery and pacing state. Their replacement needs
registration-lifetime host drivers and explicit wake admission before extraction
into the runtime; those host resources must not become runtime dependencies.

### Owner work yields between finite batches

The owner-local cross-UI runtime FIFO is cooperative: one logical operation is
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
whole-UI runtime close into a partial close. This deliberately
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

The owner-only background turn drains the addressed UI runtime's owner inbox,
then polls its async driver
without a frame. Poll-generated commands remain for the next owner opportunity.
It uses runtime incarnation authority and the shared finite FIFO budget; nested
wakes enqueue rather than recursively poll. Background operations remain lossless
and are not coalesced across other operations. Closing one presentation preserves
background work for the surviving runtime; removing the runtime revokes it.
`background_owner_pump_drains_before_polling_without_a_frame` exercises inbox
ordering, async polling after primary close and refusal after runtime replacement.

The UIKit runner starts services, execution pools and its development watcher
once per process. Its private session controller installs a real UI runtime only for
a fresh scene session; reconnect selects the retained UI runtime. Terminal discard
uses the existing addressed close path and removes the session before disposal.
The controller's generic key permits the same production ownership logic to run
with real headless UI runtimes in host tests; it is not another lifecycle reducer or a
public raw-platform capability. Root configurations can retain application-owned
state while a new session creates fresh `ViewState`. See ADR-0073 for native
attachment lifetime, panic containment and platform limits.

### Android scene rendering discharges the plugin payload lifetime obligation

The Android host calls unsafe `DevReloadHook::scene_frame` with only its
synchronous `Renderer::render_plugin_scene` callback (ADR-0108). The engine walks
borrowed layers and ignores annotated-region payloads. Cached image bytes
are concrete `Arc<Vec<u8>>`; glyph face admission copies source bytes into the
host registry's concrete `Arc<[u8]>`. The callback therefore retains no
plugin-backed trait object, including on unwind, before the hook can unload
its image. This proof is about the shipped renderer; a callback that clones
opaque scene payloads must establish its own ordering.

The callback passes the hook's pending font reset to the renderer and returns
true only for a successful rendering result. A failed rendering logs its error
and leaves the hook's reset pending. Ordinary fallback rendering restores the
renderer namespace separately; the next plugin frame therefore cannot alias
ordinary or prior-image font IDs.

### Background worker retirement preserves progress

A worker commits its newest result, then drops the result it replaced outside
the slot lock. A panic in that destructor, or in a lifecycle diagnostic, is
contained and the pump keeps running: pending input, including input submitted
by the retiring result, is still delivered. `submit` schedules the pump before
it drops the input it replaced; if scheduling is refused, the new input is
removed and dropped, as `WorkerHandle::submit` documents. A panic in the
replaced input's destructor propagates after scheduling, and a refused input in
the same call is retained rather than dropped (ADR-0127).

Pending input and pump ownership share one inbox mutex, so installing work and
reserving its pump is one transition, and so is finding the inbox empty and
releasing ownership. A pump that released ownership returns without touching
the inbox again. Host spawning and every user destructor run outside the mutex.

Discarded lifecycle panic payloads are retained (ADR-0119), as is a future
whose poll panicked (ADR-0127); healthy completion and cancellation drop the future
normally. A compute panic is reported as complete before its diagnostics run.
Pinned by `service_lifecycle_matrix`.
