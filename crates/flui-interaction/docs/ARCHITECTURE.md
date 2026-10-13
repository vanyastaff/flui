# Architecture: flui-interaction

Crate-level design notes for `flui_interaction`: subsystems, ownership, mapping decisions, thread safety, friction and outstanding refactors.

## Subsystems

The top-level modules are layers ([ADR-0217](../../../docs/adr/ADR-0217-interaction-module-layers-and-pointer-assembly.md) §1):
a module names only modules in a lower layer, and modules in one layer name none of each other.
`[package.metadata.flui.modules]` in the manifest declares them and `cargo xtask module-dag`
checks them with no exceptions. Each public item has one module path; the crate root and
`prelude` re-export the common ones.

| Layer | Module | One-paragraph description |
|---|---|---|
| 0 | `events`, `ids`, `retain`; `close`, `containment` | The owned input vocabulary and identities. `retain` decides what a failure path keeps rather than destroys; `close` holds the terminal close policy (`CloseMode`, `CloseTombstone`, `ClosePanic`) every owner shares; `containment` holds `RoutePanic`, the first failure of a synchronous dispatch phase, kept until mandatory cleanup finishes. |
| 1 | `pointer` | Pointer delivery: hit testing, the `InteractionLane` that resolves and invokes pointer routes, `PointerRouter`, `PointerCapture`, `MouseTracker` (enter/exit/hover), the `TransformGuard` stack-RAII, `PointerEventResampler` (frame-rate adaptation with a 100-event pressure threshold, preserved sequence boundaries and 1 ms minimum sample interval), its `SamplingClock`, and wheel preferences. Route resolution on Down and cached-route invocation on every Move/Up are on the per-pointer hot path (`benches/pointer_route_bench.rs`). |
| 1 | `focus` | The presentation-owned `FocusManager` (`FocusManager::new` returns an `Rc<Self>`; there is no thread-local focus state), `FocusScopeNode` / reading-order Tab traversal and directional traversal. |
| 1 | `text_input` | `TextInputOwner` and the `TextInputHandle` a text widget attaches through, and `ClipboardHandle`. |
| 2 | `gesture` | The arena: owner-local conflict resolution between competing recognizers, per-pointer members as `Weak<dyn GestureArenaMember>` in an inline-four `SmallVec`, generational slots that keep held competitions separate across pointer-ID reuse, eager acceptors that win when the arena closes. Ten recognizer types (tap, double tap, long press, drag, scale, force press, multi-tap, multi-drag, eager, tap-and-drag) and drag-axis builders, each implementing the open `GestureRecognizer` and `GestureArenaMember` traits; `ArenaMembership` names an exact weak allocation, `PrimaryContact` owns one admitted sequence, `RecognizerSet` shares ordered weak attachments with listeners. `GestureSettings`, the callback details, `DragAxis`, `VelocityTracker` (LSQ fit on 20-sample circular buffer, 100 ms horizon, 40 ms stationary gate) and its crate-internal `lsq_solver`, One Euro filters (applied by drag and scale to the positions they publish after acceptance when settings select a tuning; slop, arena and velocity read raw samples), and observability: `GestureEvent` names events, `SPAN_RECOGNIZER` and `SPAN_ARENA` name spans; rejection and terminal tracking commit local withdrawal before diagnostics, because subscribers can reenter or panic. |
| 3 | `binding` | `GestureBinding` — the owner-local composition root (ADR-0217 §2) over `pub(crate)` services that hold no reference to it or to each other: `PointerSequences` (admitted contacts and their Down routes, captures, resamplers, refusals), `MoveCoalescer`, `ScrollSequences`, `PanZoomSequences`, `MouseTracker`, `ArenaDriver` (close on Down, sweep on Up, abandon on Cancel) and `CloseCoordinator`. Every service is keyed by `InputSurface` (the window, or a surface minted by `embed_surface`); a contact stays with the surface that admitted its Down. Contact generations prevent frame-delayed samples from crossing a reused platform pointer ID. |
| above | `__runtime`, `testing` | The runtime's terminal-cleanup seam over every owner above (hidden; no semver promise), and the `testing` feature's synthetic event builders. |

## Ownership and synchronization

The synchronous pointer pipeline belongs to one `UiRuntime`. `GestureBinding`,
`GestureArena`, recognizers, pointer routes, and executable callbacks are
intentionally `!Send + !Sync`; callbacks may capture `Rc` widget state.
Strong `Rc` ownership belongs to widget state. Arena membership and cached
recognizer attachments are weak, so they cannot keep unmounted recognizers alive.

The data plane is separate. Pointer events, hit paths, IDs, transforms, and
opaque route targets remain `Send + Sync` where the renderer or embedder needs
them. Compile-time assertions in `src/lib.rs` cover only those data types.

| Site | Primitive | Reason |
|---|---|---|
| Arena slots | `Rc<RefCell<BTreeMap<..>>>` and per-slot `RefCell` | exact owner-local slot transactions; callbacks and user destruction run after releasing borrows |
| Recognizer state | `Cell` / `RefCell` behind `Rc` identity | synchronous owner-local mutation; callback configuration is immutable |
| Pointer router / interaction lane | `Rc` + `RefCell` | explicitly owner-local executable callbacks |
| Pointer resampler | `Arc<parking_lot::Mutex<ResamplerInner>>` and shared output batches | source pressure coalescing and frozen delivery prefixes; no lock crosses a callback |
| Focus manager | `Rc` + `RefCell` / `Cell`, one per presentation | owner-local focus tree and listeners |

Negative compiler fixtures reject transfer of executable recognizers, builders
and attachments to another thread. Gesture extension traits are open; their
implementations cannot change arena ordering or bypass exact membership checks.

## Mapping decisions

Local design choices and why. Each entry names the conflict, the choice, and the reference (a strategy clause, a design rule, or a precedent plan).

- **Position interpolation does not measure a sensor.** A synthetic interpolated
  position keeps the prior accepted reading's pressure, orientation, twist and
  contact metadata, including absent sensors. It must not borrow a future
  reading. Admission tracking and queued delivery are distinct: `Leave` ends
  future tracking without erasing already accepted boundaries or hover packets.
  `resampler_interpolates_on_event_time_and_never_drops_terminals` pins both
  sensor provenance and terminal/hover delivery.
- **Force recognition follows measured chronology.** Coalesced readings and the
  current reading progress pressure and motion in order; predicted readings
  never recognize a gesture. A missing pressure still checks motion, but cannot
  rewrite coordinates attached to the prior measured pressure. Callback failures
  preserve the first failure while the still-current tail progresses. Exact
  contact replacement revokes the old tail, even when the pointer ID is reused.
  `scale_and_force_press_owner_lifecycle` pins ordinary/coalesced equivalence,
  threshold excursions, missing sensors, replacement and recovery.
- **Scale continuity includes representable contraction.** Changing the contact
  set carries the prior factor into a newly measured baseline. The ratio retries
  product-first evaluation when quotient-first loses a nonzero result through
  intermediate underflow, as well as on overflow. A real zero span still reports
  zero; an unrepresentable final result follows the existing finite-publication
  policy. `scale_and_force_press_owner_lifecycle` pins huge rebaselines, tiny
  contractions, actual zero and fresh-contact recovery through public callbacks.

- **Standalone admission differs from mounted admission.** A resampler's
  100-event threshold is a pressure policy, not a hard memory limit. Admission
  preserves `Down`, `Up`, `Cancel`, `Enter` and `Leave` in order; pressure
  coalesces adjacent moves or removes the oldest non-boundary event. If only
  boundaries remain, the next event is accepted beyond the threshold. Standalone
  callers must pace `sample` or `stop`; silently losing an accepted terminal
  would leave downstream recognition active. Clones share one queue, while
  separately constructed resamplers remain independent even with equal pointer
  IDs. `resampler_interpolates_on_event_time_and_never_drops_terminals` covers
  repeated-contact boundary pressure, mixed moves, shared delivery and fresh
  recovery. `GestureArena::with_capacity` reserves deferred-resolution storage,
  not contact admission; `gesture_lifecycle_matrix` admits and settles contacts
  beyond both that reservation and the binding's limit.
- **Prepared output survives a failed observer.** Sampling commits source state
  into shared output batches before calling user code. Each packet is consumed
  before its callback; a callback panic leaves the unvisited tail deliverable by
  a subsequent healthy sample or stop, including through a clone after stop
  reset tracking. Nested driving consumes older output before newly prepared
  packets, while an outer frozen prefix cannot acquire newer work. The binding
  uses that same lazy delivery prefix before button edges and observing input.
  Explicit `clear` cancels source and unconsumed output; dropping an output
  iterator alone preserves debt. `has_pending_events` and `pending_event_count`
  include prepared output. Terminal contact withdrawal cancels that contact's
  already-prepared prefix before terminal callbacks, while still-queued source
  samples retain their terminal flush. A reused pointer ID owns a fresh queue;
  reentrant withdrawal cannot flush the old frozen tail into its replacement.
  The public
  `resampler_interpolates_on_event_time_and_never_drops_terminals` pins failed
  Down delivery with Up/Cancel tails, shared and independent recovery, clear,
  reentrant Move ordering before button or pending-input observation, and
  Up/Cancel withdrawal with same-ID recovery during frame/input driving.
  The source pressure threshold does not cap prepared output debt; repeated
  observer failures require subsequent healthy driving or explicit cancellation.
  Shared queues commit each packet once; concurrent driver callbacks have no
  completion-order guarantee, so ordered UI delivery remains owner-synchronous.
  Source admission reserves reusable shared batch storage. A live frozen prefix
  prevents its batch identity from being reused even after cancellation; retiring
  prefixes releases unused pool storage while keeping one idle buffer. The public
  `resolved_route_move_invocation_allocates_no_heap_after_setup` pins unchanged-time
  measured Sample/Stop delivery without allocation after admission and the
  saturated source enqueue allocation bound.
- **The binding owns mounted gesture progression.** `GestureBinding` admits
  at most 32 simultaneous contacts, retains each admitted Down route, closes
  its arena after routing Down, and sweeps or abandons its exact generation on
  terminal input. It flushes accepted motion before terminal delivery and owns
  deferred verdict/deadline progression. `GestureArenaScope` supplies that
  binding-driven arena to widgets; recognizers below it do not self-sweep.
  Standalone drivers close after Down routing, sweep after Up (abandon on
  Cancel), drain deferred resolutions and poll deadlines when due. Holds must
  eventually release their exact entry handle, including after pointer reuse;
  reserving capacity does not remove those obligations. The public
  `gesture_lifecycle_matrix` covers held release and pointer reuse, while
  `binding_input_contract_matrix` covers capped contacts, refused tails and
  lifecycle reset. The binding's contact cap does not limit members per contact.
- **Terminal lane retirement commits before user destruction (ADR-0127).**
  Closing withdraws executable callbacks and opaque route ownership before
  retiring them outside borrows. Healthy retirement releases resources; after
  the first failure or during unwind, remaining owners that could execute user
  code are retained. Non-last owner-local `Rc` clones can still be released.
  Saved dispatch handles become stale, while another live lane remains usable.
  `lane_terminal_retirement_contract` covers independent storage families,
  competing failures, reentry, unwind retention and surviving-lane recovery.
  Every target snapshot also carries terminal lane authority, including raw
  registrations without a presentation owner. `lane_snapshot_authority_contract`
  pins cached mouse exits after lane closure and revocation between callbacks
  already admitted to one round. Final framework snapshots retain withdrawn
  captures after a preserving close; external non-last callback clones remain
  releasable. Fresh stale-handle values retire ordinarily after the close window
  returns, while rejected ownership during that window follows its failure mode.
  Containment cannot rescue one opaque aggregate that double-panics inside its
  own destruction before control returns to the framework.
- **Dispatch identity does not imply activation authority.** A saved dispatch
  handle carries an exact lane ticket but runs only while that lane is entered
  on its owner thread. `UiRuntime::enter` establishes this scope for mounted
  consumers; standalone composition uses `InteractionLane::enter`. Nested
  activation restores the previous ticket on return or unwind. An inactive,
  foreign, wrong-thread or closed lane is rejected before invoking its targets,
  rather than borrowing another runtime's active lane. `lane_capability_matrix`
  pins those authority boundaries and stale capabilities after owner recreation.

- **Diagnostics participate in the subsystem's ownership boundary.** Router
  mutations and ignored sampling windows release their borrow or mutex before
  tracing invokes user code. Focus commits primary/history and enters its
  notification round before diagnostics, then completes observers and accepted
  FIFO requests before resuming the first failure. The public
  `binding_input_contract_matrix`,
  `resampler_interpolates_on_event_time_and_never_drops_terminals` and
  `caught_callback_failures_leave_captures_with_their_owner` cover diagnostic
  reentry, competing failures and subsequent healthy delivery.
- **A router snapshot identifies a registration, independently of its callback.**
  Removing a callback removes its duplicate registrations; adding the same
  callback creates new registrations eligible for the next event. Private
  retained identities cannot alias an outstanding snapshot. The
  `binding_input_contract_matrix` pins pointer/global duplicates, removal,
  re-admission and accepted healthy tails after failure.
- **Cursor publication belongs to the latest physical source.** Device-local
  enter/exit rounds remain independent. Ambient refresh updates the current
  source's window cursor without handing ownership to a stationary device by
  iteration order. Publication debt differs from an in-flight hook and from
  acknowledged output; only successful current delivery acknowledges it.
  Reentry and replacement preserve newer debt. Removing the owner requests the
  arrow with its actual source metadata and permits retry without probing the
  removed source. Ambient probes revalidate their exact device observation
  after user code, so stale results cannot replace a newer physical reading or
  a re-admitted source. `mouse_tracking_ordering_and_cursor_deferral` pins these
  boundaries, same-position reentry, callback failure and healthy recovery.
- **Checked positions do not guarantee representable derived motion.**
  TapAndDrag checks initial, incremental and measured historical displacement
  before recognition, velocity sampling or publication;
  overflow cancels the attempt rather than fabricating a clamped delta.
  `tap_and_drag_resolves_through_the_shared_arena` covers these boundaries,
  cancellation failure and finite same-pointer recovery. Scale commits its
  admitted contact before calling the user clock outside its state borrow,
  then revalidates the exact generation. `public_recognizer_extension_contracts`
  covers clock cancellation, repeated-pointer replacement, failure and recovery.

- **Render hit paths contain identities, not executable target objects.**
  Rendering protocols produce `RenderId` paths and data-only owner-lane targets;
  `InteractionLane` resolves them and `GestureBinding` retains pointer routes.
  A second target trait or generic dispatcher would duplicate this ownership
  boundary without providing a producer. `transformed_entry_receives_local_samples_and_deltas`
  pins dispatch through the actual lane, including local geometry.
- **Exact root identity borrows complete pointer readings.**
  `HitTestResult::add` composes an identity for targets in root space. Resolving
  that exact matrix as an unlocalized route keeps measured and predicted
  histories borrowed and preserves source metadata. A near-identity matrix
  still follows checked localization; a tolerance would erase authored motion.
  Transform classification borrows the optional matrix until a nonidentity
  transform is admitted. Only that branch needs an owned matrix in the cached
  route; inspecting absence or exact identity does not require copying the
  whole optional payload first.
  `resolved_route_move_invocation_allocates_no_heap_after_setup` covers real
  identity, translated and near-identity hit paths, complete sample families
  and their allocation contracts.
- **Explicit capture belongs to one admitted Down generation (ADR-0164).**
  A real target's `PointerDispatch::capture` returns a non-Clone weak token.
  The first claimant selects later target delivery while the original Down
  observation round finishes unchanged. Release commits capture-loss debt
  before waking the exact presentation, outside all borrows; wake failure
  cannot erase it. Already accepted motion and committed frame batches finish
  before the one `CaptureLost`, and newer released tails are refused by pointer
  and optional device identity. Missing device identity cannot distinguish
  unreported sources. Listener unmount removes future hit-test admission but
  preserves the cached contact's terminal obligation; presentation close has
  its separate retirement policy. This controls logical routing and does not
  advertise an OS capture-release operation. `explicit_pointer_capture_contract`
  and widget `listener_unmount_preserves_one_captured_contact_terminal` pin the
  lifecycle, failure and accepted-motion boundaries.
- **An arrow request differs from deferring a cursor (ADR-0158).** `CursorRequest::Defer`
  leaves the choice to the next hit target; `Icon(CursorIcon::Default)` selects
  the arrow even when an ancestor asks for another icon. Render objects without
  a cursor contribution and unconfigured `MouseRegion` widgets defer.
  `explicit_arrow_cursor_wins` pins both hit-path resolution and tracker delivery.
- **Hover annotations survive until every device leaves.** The tracker keeps a
  shared resolved annotation while any device remains in its region. Devices
  refresh in identity order, delivering each committed callback round before
  probing the next device. A later reentrant probe therefore observes completed
  prior transitions, including their required exits. Each failed hit test
  preserves that device's prior
  state for retry, and every committed callback batch runs before the first
  failure resumes. Replaced and departed captures retire outside the tracker
  borrow under ADR-0127. `shared_region_exit_per_device`,
  `ambient_refresh_contains_each_device` (including later-probe reentry) and
  `released_region_destructor_reenters_tracker` pin these contracts.
- **Hover payload retirement preserves delivery and first failure (ADR-0127).**
  Queued replacement commits its newer movement before the outgoing hit path
  retires. Each entry detaches metadata before destruction and retains it
  during unwind. Frame delivery explicitly retires metadata after an already
  caught callback failure, while accepted sibling movements still run.
  `binding_input_contract_matrix` pins healthy destruction, callback and
  metadata retirement competition, reentrant replacement and fresh recovery.
  Containment ends at each opaque payload; its own double-panicking destructor
  cannot be rescued by the framework path.

- **Focus node identities are never reissued.** The allocator admits its final nonzero identity once and then refuses every new `FocusNode` with a panic, permanently, even after that panic is caught. Wrapping would hand a retired identity, and the focus authority it names, to a new node; refusing keeps every attached node's requests and listeners intact.
- **Configure before sharing; cancel before releasing.** Builders return `Rc`
  recognizers whose callbacks are immutable. Explicit `cancel` delivers an
  active sequence's cancellation and leaves admission reusable. Last-owner Drop
  silently withdraws exact membership and retires captures without gesture
  callbacks. The `PrimaryContact` helper refuses overlapping admission and
  gives each sequence a non-reissued `ContactId`; multi-contact recognizers use
  `ArenaMembership` independently for each pointer.
  `public_recognizer_extension_contracts` pins overlapping admission, settings
  freezing, cancellation of every owner, silent Drop and later recovery.
- **Multi-tap is a simultaneous contact group.** `MultiTapGestureRecognizer`
  recognizes the configured number of contacts only after all have released
  within their own slop. Either release order completes once; an unrelated
  cancellation leaves the group intact, while a tracked cancellation retires
  the attempt and allows the next group. This is the policy documented in
  [GESTURES.md](GESTURES.md), pinned by
  `multi_contact_events_keep_independent_pointer_identity`.
- **One owned input vocabulary (ADR-0143).** Platform dispatch, interaction,
  runtime replay and widget callbacks share `flui-platform-api`'s pointer and
  keyboard types. Pointer and hardware IDs remain distinct; absent device or
  sensor metadata is not fabricated. Localized measured and predicted sample
  families preserve their source metadata. Listener callbacks borrow this
  same source instead of flattening it into another enum.
  `pointer_source_contracts` pins mouse source identity fallback and reentry;
  the mounted `pointer_delivery_preserves_source_and_sample_families` and
  `scroll_claim_preserves_owned_source_units_and_phase` rows pin the widget edge.
- **Measured excursions survive frame coalescing.** A delivered Move's ordered
  measured history and current position participate in slop admission. Returning
  to the contact origin within one frame cannot restore tap viability or hide a
  drag/scale threshold crossing. Tap, double-tap, long-press, multi-tap, drag,
  multi-drag, tap-and-drag and scale use borrowed measured positions; predictions
  never admit or cancel a gesture. Geometry callbacks retain their frame-current
  local/root positions and event timeline instead of replaying one callback per
  historical sample. Velocity trackers still consume the measured timestamps
  within their estimator's own retention window. Recognition still follows
  arena verdicts, and callback reentry rechecks the exact contact generation.
  `tap_and_drag_resolves_through_the_shared_arena` pins each recognizer's isolated
  excursion, queued/authored history equivalence, prediction exclusion, reused-ID
  recovery and cancellation from a coalesced start callback.
  `resampler_interpolates_on_event_time_and_never_drops_terminals` pins preservation
  of three packets' six measured readings and only the newest prediction family.
- **Resampler delivery preserves owned history storage.** An unchanged event
  timestamp reuses its measured and predicted vectors; raising a timestamp still
  uses the checked builders to exclude predictions preceding the new current
  reading. Saturated queues retain at most 100 historical readings per move in
  place after canonical coalescing validates full pointer identity. Coalescing
  transfers the retiring packet's history storage; checked chronological
  boundaries avoid re-sorting, while overlapping timestamps retain canonical
  filtering and stable ordering. The
  counting-allocator family
  `resolved_route_move_invocation_allocates_no_heap_after_setup` pins owning
  Sample/Stop delivery, raised-time filtering and saturated admission with 199
  delivered measured readings plus the newest predictions.
- **`TapButton` is a typed enum, not integer button constants.** `TapButton` (`src/gesture/recognizers/tap.rs`) maps pointer buttons explicitly through `from_pointer_button`, so the type system enforces the choice. It is `#[non_exhaustive]` so a future fourth button slot can be added without breaking downstream.
- **Weak arena membership.** The inline-four member list stores weak identities,
  not lifetime ownership. Dead members withdraw; queued verdicts recheck
  liveness at each invocation. `gesture_arena_bench` measures empty and busy
  admission separately. `gesture_lifecycle_matrix` pins withdrawal, generation
  isolation, first-failure authority and subsequent recovery.
  The generated operation sequences in
  `arena_settles_every_member_exactly_once` assert terminal verdict uniqueness
  and settlement across pointer reuse.
- **Gesture extension points are open.** External recognizers implement the same
  dyn-compatible `GestureRecognizer` and `GestureArenaMember` traits as built-ins.
  Arbitration and deadlines use one path, with no marker-trait bridge losing
  deadline behavior. `public_recognizer_extension_contracts` drives external
  implementations through the same attachment and arbitration path.
  Hit-test contracts are a separate surface.
- **`pending_up` deferral for `on_tap_up`.** Before the fix, `handle_tap_up` fired `on_tap_up` and `on_tap` unconditionally on pointer up, even though every arena member receives Up events. The fix stores a `pending_up` until `accept_gesture` confirms arena victory; only the eventual winner fires the user callback. The same pattern was extended to per-button slots.
- **A fired long press resolves its own arena.** Deadline delivery commits the
  started state and disarms the deadline before accepting exact membership and
  invoking callbacks outside the state borrow. A repeated poll cannot refire
  the sequence. The arena queries `deadline()` and supplies its single clock
  reading to `poll_deadline(now)` only when due.
- **Double-tap debounce starts at the first release.** An otherwise eligible
  second Down must arrive at least 40 ms after the first Up on the arena's owner
  clock, independently of how long the first contact stayed down. Earlier Downs
  are ignored while the first tap's held verdict and original timeout remain
  pending; lifting that bounced contact after the boundary cannot admit it
  retroactively. Exact 40 ms is eligible. Timeout and inter-tap distance retain
  their separate frozen-settings policy. A clock callback can retire or replace
  the contact, so the first-release snapshot commits only after checking the
  exact contact identity again. `tap_and_drag_resolves_through_the_shared_arena`
  pins 39/40 ms admission, both mouse and touch, a held first contact and reused-ID
  recovery; `tap_builder_lifecycle_contract` pins cancellation and later reuse.
- **Gesture settings belong to the admitted sequence.** A presentation has one
  `GestureSettingsSource` producer and supplies readonly owner-local providers.
  Authored `GestureSettings` remain fixed. New contacts observe the provider;
  active contacts, drag handoff groups, scale sessions and consecutive-tap
  candidates retain their admitted policy. Publishing a replacement invokes no
  callback and requests no frame. `admitted_gesture_settings_contract` exercises
  retained, next-contact and restored controls through gesture callbacks.
  Exact presentation geometry keeps mouse rectangles per axis; observed touch
  distances preserve authored hit-to-pan and hit-to-span ratios. A zero baseline
  hit tier keeps its authored dependent tiers, while an unrepresentable derived
  tier returns a recoverable error. Native mouse click timing begins at the
  first Down; authored and touch intervals begin at the first Up. The independent
  40 ms bounce guard remains measured from the first Up.
  Consecutive contacts must share pointer kind and typed device identity;
  two absent device identities use kind-local matching, while a known device
  cannot match an unknown one. Ordinary successive touch contacts may use
  different pointer IDs. Measured history counts toward drift; predictions do
  not. Raw terminal velocity preserves finite measured components independently
  of the admitted fling bounds. An unrepresentable component estimate is refused
  as zero without erasing the finite orthogonal component;
  `nonrepresentable_component_estimate_is_refused` pins both axes across all four
  estimators, admitted fling bounds and the next healthy contact. Free-drag
  terminal scalar magnitude alone uses `f64::MAX` when its norm
  cannot be represented; the finite raw vector and derived directional fling
  remain available separately.
  `DragEndDetails::fling_velocity` and `ScaleEndDetails::focal_fling_velocity`
  apply the admitted pixel-speed bounds once at terminal delivery. Scale's
  dimensionless velocity remains independent of those bounds. Native scale
  stages its profile at Begin without claiming delivery or invoking callbacks;
  `PanZoomDisposition` distinguishes that admission from a refused Begin and
  from recognized handling. The mounting owner keeps a refused native session
  refused until its terminal event, while an Update without Begin remains an
  independent relative step.
- **Native cached routes require current admission before dispatch.** Fresh
  hit testing or raw observation can admit a replacement Start for the same
  source and timestamp. A superseded Start or Update cannot invoke the cached
  actor; rejecting its claim after invocation would already mutate the newer
  session. `binding_input_contract_matrix` covers both observation paths,
  competing observation failure and subsequent Update/End recovery.
- **Focus scope identity is explicit.** A `FocusScopeNode` owns an inner `FocusNode`, and that backing node carries a `Weak<FocusScopeNode>` owner link. This keeps enclosing-scope lookup, focused-child history, and `FocusManager::focus_next` / `focus_previous` rooted in the same tree instead of relying on a parallel manager structure. `descendants_are_focusable=false` gates descendant requests; a true-to-false transition evicts focus held by the node or its subtree while leaving the node eligible for a later explicit request. FLUI clears primary focus to `None` rather than selecting a previously focused child.
- **`gesture`'s `lsq_solver` is crate-internal.** `VelocityTracker` is its only user; the resampler interpolates linearly and does not fit a polynomial.
- **Observability is crate-public.** `pub mod observability` exports stable `GestureEvent` spellings, component-name constants, and `pointer_event_kind`. `flui-app` configures a generic subscriber; gesture-specific devtools consumption requires its own integration. `stable_recognizer_observability_kinds_reach_the_subscriber` pins admission and dispatch fields through public recognizer calls.
- **Synchronous FIFO reentrant-focus queue vs six different reference shapes, none of them adopted as-is.** Surveyed in [`.rust-studio/specs/1040-ordered-focus-notifications/survey.md`](https://github.com/vanyastaff/flui/blob/e30ab7194d50ac1c11ffe17c59230958d2fbeecd/.rust-studio/specs/1040-ordered-focus-notifications/survey.md) (issue #1040). Six engines/frameworks solve "a focus listener changes focus again" six different ways:
  - **Flutter** (`focus_manager.dart`, `FocusManager.applyFocusChangesIfNeeded` / `_markedForFocus` / `_markNextFocus`): defers every `requestFocus`/`unfocus` to a microtask (`_markNeedsUpdate` → `scheduleMicrotask`) and collapses every request made before that microtask runs into one `_markedForFocus` slot — last request wins, and `_markNextFocus` clears the pending mark outright when a request names the already-current node. Chronological by construction and every listener sees a coherent order, at the cost of up to one frame of lag and a reentrant chain that livelocks across microtasks (still yielding frames, so it never hangs the caller). The deferral traces to flutter/flutter#9074 (2017): *"Focus notifications trigger[ed] by tree mutations are now delayed by one frame, which is necessary to handle certain complex tree mutations"* — reentrancy safety is a side effect of that motive, not the reason it was built.
  - **Jetpack Compose** (`androidx.compose.ui.focus.FocusTransactionManager`): a reentrant `requestFocus` from inside `onFocusChanged` **cancels and reverts** the in-flight transaction before starting the new one. Compose later had to add a dedicated cancellation-notification path (b/319817633, b/312524818) specifically because a silent revert left observers holding stale state — a second notification kind existing only to patch the first one's blind spot.
  - **WHATWG HTML**: browsers apply a nested `focus()` call synchronously and dispatch its `blur`/`focus` events at commit time (chronological by construction, with no lag). The spec's own "locked for focus" reentrancy guard was never implemented by any engine and was formally removed (whatwg/html#11182, 2025) — the only bound on a reentrant ping-pong is the engine's stack-recursion limit.
  - **GPUI** (`zed-industries/zed`, `crates/gpui/src/window.rs`, `Window::focus` / `Window::draw`): `focus` only records intent (a target handle plus a `focus_generation` bump); the actual focus-changed event is computed at draw time, as a diff between the previous and current rendered frame's focus path. A listener that moves focus during that diff is handled by deferring to the *next* frame rather than recursing. Their own source comment records exactly the incident this crate's drain budget exists to prevent: *"scheduling a redraw below would dispatch focus-lost again, looping forever"* — they fixed it by narrowing what counts as listener-caused movement, not by a budget.
  - **Masonry/Xilem** (`masonry_core/src/passes/update.rs`, `run_update_focus_pass`): `request_focus` writes a pending `next_focused_widget` slot; the update pass commits it and delivers `FocusChanged` to the old and new widget — last-wins, applied in a pass after event handling, the same family of solutions as Flutter's.
  - **Qt** (widgets): fully synchronous nested dispatch — `setFocus` called from inside `focusInEvent` simply recurses, and `QApplication::focusChanged` fires per change with no engine-side reentrancy bound at all; the escape hatch it gives a handler is `QFocusEvent::reason()`, so code can recognize and decline to react to a focus change it caused itself. Real infinite-focus-loop reports exist in the wild for exactly this shape.

  FLUI's synchronous `request_focus`/`unfocus` cannot defer admission to an unavailable microtask. `FocusManager` queues reentrant requests and revalidates them immediately before applying each FIFO entry: attached, focusable, and still owned by this manager. Each application commits state, publishes node observers, then publishes manager observers. A shared containment accumulator completes every committed round and drains accepted requests before propagating the first listener failure; a competing failure does not erase delivery debt. This preserves chronological edges without recursive application or transaction rollback. The public `caught_callback_failures_leave_captures_with_their_owner` table pins node and manager origins, queued request/unfocus ordering, competing failures, observer replacement, and healthy recovery. `finish_node_replacement` participates in the same ordering, publishing a manager edge only when primary identity changes. Terminal `close` retains its distinct policy: commit terminal state first, stop final node notifications after failure, and omit manager publication while an ordinary notification is in flight. The synchronous drain remains bounded to 32 committed applications per outermost call; it warns once and cancels the remaining tail (`ping_pong_listeners_are_bounded_and_warned`). Dispatch snapshots name listener registrations and recheck membership before invocation, so removal takes effect immediately and newly added listeners wait for a later snapshot.

- **Committed focus notifications finish before propagating failure.** Node and manager observers share one containment accumulator across the outer request and its accepted FIFO tail. Every still-registered observer in each round runs, even after an earlier observer panics; callback snapshots and canceled queued nodes retire outside borrows and preserve the first failure. Diagnostic subscribers are contained by the same accumulator. Reentrant registration changes affect subsequent snapshots, and a removed listener is skipped. The public `caught_callback_failures_leave_captures_with_their_owner` table pins node-only failures, competing manager failures, node/manager competition, self-replacement, queued requests, canceled-target retirement, diagnostic failures, and the next healthy focus operation. Terminal close notifications retain their separate stop-on-failure policy. The depth guard restores only depth: it neither cancels accepted debt nor executes user code on an unexpected unwind.

- **Nested focus cleanup inherits a caught observer failure.** A scoped owner-local `CloseMode` is established before subsequent observers or queued-retirement diagnostics run, and updated immediately after containment before callback snapshots retire. A later observer calling `close` therefore commits terminal state while retaining opaque captures instead of starting ordinary destruction under an earlier saved failure. Nested scopes restore the previous mode; a live owner's later healthy operations do not inherit a policy from a failure its caller already caught. The public `caught_callback_failures_leave_captures_with_their_owner` table pins healthy and panicking nested-close captures, the exact first observer failure, terminal state, stale requests, and repeated close.

- **Scoped hit-test transforms survive caught panics.** `with_paint_offset` and `with_paint_transform` restore their entry transform depth through `TransformGuard`, whether the callback returns or unwinds. Pending transform parts may have become globalized while entries were added, so rollback restores the combined depth rather than blindly popping one part. The rendering consumer `hit_test_matrix` catches a transformed descendant failure and checks the healthy sibling's emitted local coordinates. Raw pushes still require balanced pops; scopes do not authorize removing their ancestors' transforms.

## Testing strategy

| Command | Purpose |
|---|---|
| `cargo test -p flui-interaction --all-features` | Unit, integration, feature-gated testing helpers, and doctests across arena / recognisers / processing / routing / timer. |
| `cargo test --doc -p flui-interaction` | Public source-documentation examples. Standalone Markdown is tested directly with the command below. |
| `cargo bench -p flui-interaction` | Criterion workloads and saved-baseline comparisons (see [PERFORMANCE.md](PERFORMANCE.md)). |
| `cargo clippy -p flui-interaction --lib --tests --benches -- -D warnings` | Lint gate — zero warnings. |
| `cargo fmt -p flui-interaction --check` | Format gate. |

### Standalone Markdown examples

Run from the workspace root in Bash with `jq` installed. Cargo's JSON output identifies
the exact libraries built for this checkout and feature selection; do not reuse saved
library paths or select arbitrary files from `target/debug/deps`. The direct rustdoc
invocations run the Markdown examples without adding documentation-only source modules.

```bash
set -euo pipefail
artifacts=$(mktemp)
trap 'rm -f "$artifacts"' EXIT
cargo build --locked -p flui-interaction --lib --features testing \
    --message-format=json > "$artifacts"

externs=()
for crate in flui_interaction flui_foundation flui_platform_api web_time; do
    library=$(jq -ser --arg name "$crate" '
        [ .[] | select(.reason == "compiler-artifact" and .target.name == $name)
          | .filenames[] | select(endswith(".rlib")) ] | unique
        | if length == 1 then .[0] | gsub("\\\\"; "/")
          else error("expected one current library artifact") end
    ' "$artifacts")
    externs+=(--extern "$crate=$library")
    if [[ "$crate" == flui_interaction ]]; then
        dependency_dir="$(dirname "$library")/deps"
    fi
done

for document in crates/flui-interaction/README.md \
    crates/flui-interaction/docs/{GESTURES,ARCHITECTURE,PERFORMANCE,HIT_TESTING}.md; do
    rustdoc --test "$document" --edition=2024 \
        -L "dependency=$dependency_dir" "${externs[@]}"
done
```

`cargo test --doc` and these direct Markdown runs cover different inputs. This command
does not install a new gate; retain the document-by-document results with the checked
revision. Inspect source inclusions with `rg 'include_str!' crates/flui-interaction/src`.
The classifier treats crate READMEs and nested crate documentation as source-impacting
paths, so this guide needs no `DOCS_ONLY` exemption.

### Native input evidence

The public native window contract lives in
[`flui-platform/tests/contract.rs`](../../flui-platform/tests/contract.rs).
On Windows, select one existing child row through its environment variable;
the top-level test exits after running that row. In PowerShell:

```powershell
$env:FLUI_NATIVE_WINDOW_CONTRACT_CHILD = 'fractional_native_wheel_packets_preserve_observed_precision_and_source'
try {
    cargo nextest run --locked -p flui-platform --test platform_it test_window_lifecycle_contract --no-capture --build-jobs 6 -j 1
} finally {
    Remove-Item Env:FLUI_NATIVE_WINDOW_CONTRACT_CHILD
}
```

This row posts by-value mouse and wheel messages to an owned hidden HWND and
pumps the native window procedure. It observes source identity, signed units
and fractional precision without foreground activation or global injection.
It proves message translation and delivery, not physical wheel classification.
The separate `synthetic_touch_and_pen_reach_native_pointer_dispatch` row uses
Windows synthetic pointer injection into a shown owned window. It requires an
interactive, unobscured desktop and successful activation; `CANNOT_VERIFY`
must remain an unavailable result, never be reported as a passed input check.
Synthetic pen/touch execution does not certify a physical pen or touch sensor.

`cargo xtask device windows-input` drives a real counter through OS mouse and
keyboard input and UI Automation, with foreground and hit-target checks. It
requires Windows with an interactive desktop and does not exercise pen/touch.
`cargo xtask live-smoke` uses XTEST under Xvfb on Linux; its `--wayland` variant
uses headless weston for teardown. Both variants are Linux-only; an explicit
skip on another host is not execution evidence. Cross-typecheck and wasm-check
are compilation evidence only.

Retain the exact revision, command, exit status, verdict and output outside the
repository. For physical claims, also retain device/backend identity and the
actual exercise: contact/hover, sensor values, multi-contact, capture loss and
terminal recovery as applicable. Input traces need the provenance described
in [PERFORMANCE.md](PERFORMANCE.md#trace-provenance-and-replay).

## Observability

The observability substrate lives in [`gesture`](../src/gesture/observability.rs) (exported as
`flui_interaction::gesture::{GestureEvent, SPAN_RECOGNIZER, SPAN_ARENA, pointer_event_kind}` and
re-exported at the crate root). Arena admission and resolution emit tracing spans and typed
gesture events. Configure your subscriber at the app boundary; the crate does not install one.
Tracing targets are module paths. Filter via
`RUST_LOG=flui_interaction::gesture::arena=debug,flui_interaction::gesture::recognizers=trace`.

## Friction log

- **`is_resolved(pointer)` is a state query.** Callback failure is handled at
  resolution boundaries. Catching an unwind is not a query for poisoned state.
- **`make_*_event` test helpers are `#[cfg(any(test, feature = "testing"))]`.** The benches depend on the `testing` feature being enabled in `dev-dependencies`. Documented at `Cargo.toml`; the gates will surface any missing opt-in.

## Outstanding refactors

- **Documentation validation includes source and Markdown.** Find remaining
  excluded Rust examples with `rg 'rust,ignore' crates/flui-interaction/src`;
  counts depend on the checked revision. An example is executable only when
  its source is tested by `cargo test --doc` or its Markdown document is passed
  directly to `rustdoc --test` as shown above.
- **Concurrency models must match ownership.** The executable arena is
  owner-local; a parallel `add` / `resolve` model would test an unsupported
  execution contract. Deferred data-plane synchronization needs its own model.
- **Bench fidelity: measured input bursts.** Current benches use synthetic events. The existing owned-event resampler replay is sufficient for explicit fixtures; physical throughput and latency claims additionally require device provenance and recorded arrival/frame schedules. See [trace provenance and replay](PERFORMANCE.md#trace-provenance-and-replay) before extending the workload. No general-purpose recording format is required for a literal owned-event fixture.
- **Re-export the `pub mod observability` at `crate::prelude`** once the devtools substrate is stable — currently only the `GestureEvent` / `SPAN_*` items are re-exported at the crate root.

## Index of in-crate companion documents

Subsystem-level companions:

- [`docs/GESTURES.md`](GESTURES.md) — gesture catalogue.
- [`docs/HIT_TESTING.md`](HIT_TESTING.md) — hit-test walk.
- [`docs/PERFORMANCE.md`](PERFORMANCE.md) — complexity, bounds and the Criterion benches.


## Accepted drag terminal reason

`DragEndDetails::reason` reports `GestureEndReason::Completed` for pointer Up and
`Cancelled` for an accepted pointer Cancel (ADR-0112). The end callback remains
common to both outcomes; pre-acceptance rejection still reports the cancel
callback. Velocity retains its measured value in either outcome, so consumers
choose cancellation policy from the reason rather than inferring it from a
zero velocity. Contact state and tracking retire before callback delivery,
without holding the drag-state guard. The widget consumer row
`horizontal_drag_pointer_cancel_after_acceptance_ends_and_does_not_wedge_the_detector`
observes actual cancellation, subsequent release and both reasons.

## Hit transform admission

`HitTestResult::with_paint_transform` returns `Option<R>` (ADR-0113). It obtains
an admitted inverse through `Matrix4::try_inverse` before changing the stack or
calling the descendant. Refusal publishes no descendant entries; callers can
map `None` to a subtree miss. The callback capture is retired normally even on
refusal; the helper introduces no callback panic containment. Successful scopes
keep the existing combined-depth unwind guard. The public
`hit_test_transform_admission` family covers singular, non-finite and computed
range refusals, tiny finite scales, a healthy scope after each refusal, and
caught descendant failure after entries have globalized the transform stack.
The row `refused_callback_retirement_failure_preserves_the_next_scope` owns a
callback capture whose ordinary destructor panics: refusal does not invoke the
callback, the retirement failure propagates, and a healthy sibling still uses
the parent coordinate space. It does not promise containment of multiple
panicking destructors within one opaque capture.

## Pointer route capture retirement

`PointerRouter::remove_all_routes`, `clear_global_handlers` and `clear` detach
the removed callbacks and release their registry borrows before any capture is
destroyed. A capture destructor
may query or remove the same pointer and register its next route; that new route
survives the outer removal. An ordinary destructor panic propagates after the
registry transaction commits. Framework-owned callback collections retire each
callback independently. After the first failure, the remaining opaque callback
handles are retained; during an active unwind all removed callback handles are
retained. This exceptional-path retention prevents a later destructor from
competing with the first failure. It does not contain multiple panicking
destructors inside a single opaque callback capture. `remove_route` and
`remove_global_handler` retain the caller's callback handle.
Final owner destruction detaches both registries through exclusive `get_mut`
access before applying the same retirement fence across pointer and global
callbacks. No diagnostics run between detachment and retirement. A weak handle
cannot upgrade during last-owner destruction; callback cleanup must accept that
absent-owner fallback instead of trying to resurrect the retired owner.
The public binding test
`pointer_route_retirement_reenters_and_preserves_the_next_contact` covers normal
capture reentry and a panic after reentry, then delivers the next contact through
`GestureBinding` to the replacement route.
`pointer_router_competing_retirement_preserves_first_failure_and_recovery`
uses bounded child processes for each collection-removal operation, with one or
two separately stored hostile captures and removal during an active unwind. It
checks the original failure, retained tail and actual next-contact delivery.
The same bounded family covers last-owner destruction with one hostile capture,
pointer/global capture competition and an active unwind, including weak-owner
refusal during retirement.

## Focus traversal policy snapshots

Traversal snapshots the current `Rc` policy and releases the policy cell before
calling user sorting code. A policy may replace itself during sorting; the
replacement applies to the next traversal, while the in-flight key uses its
original policy's order. Outgoing policy destruction also runs without a
policy-cell borrow. The public Tab path and policy/destructor reentry are pinned
by `tab_and_shift_tab_move_the_focus_through_the_actions_chain`.

Sorting borrows policy and candidate ownership held outside its containment
boundary. Retirement preserves the first sorting or destructor failure and
retains later opaque owners. Traversal during an existing unwind returns no
order without invoking user sorting code. The public Tab and Shift-Tab row
`tab_traversal_preserves_failure_before_policy_and_candidate_retirement` covers
policy replacement, last-owner retirement competition and subsequent traversal.

## Terminal focus ownership

Global keyboard registrations have two lifetimes. `add_global_key_handler`
installs a presentation-lived callback, withdrawn by clear-all or owner teardown.
`register_global_key_handler` returns a non-cloneable `GlobalKeyRegistration`
whose Drop withdraws exactly its registration. The weak owner prevents the token
from retaining a presentation. This scoped capability is part of the supported
consumer interface even when no framework widget needs a presentation-global
handler; widget key handlers remain node-owned `FocusNodeRegistration`s.

Withdrawal commits before callback retirement and runs no user code under a
borrow. Dispatch snapshots contain registration identities and recheck live
membership: withdrawing a later handler skips it immediately; re-registering
the same callback creates a new identity that waits until the next dispatch.
Closed owners return `GlobalKeyRegistrationError::OwnerClosed`; identity
exhaustion permanently returns `IdentityExhausted`, preserving older handlers.
Healthy rejection retires incoming captures; an earlier unwind or caught focus
observer failure selects the owner's existing preservation policy.
`scoped_global_key_registration_contracts` covers independent owners, snapshot
revocation, weak ownership, rejection, hostile retirement, reentry, competing
observer failures and recovery. The private terminal-counter row of
`focus_traversal_matrix` proves identities cannot be reused.
`manager_listener_withdrawal_preserves_a_caught_failure` pins the same cleanup
policy for individual and bulk focus-manager callback withdrawal.
Nested global key dispatch inherits a focus notification's saved failure before
invoking a callback, and keeps that scoped policy until the protected callback
snapshot retires. Withdrawing the current registration cannot make its last
dispatch clone run hostile captures under an ordinary cleanup policy. The
nested-dispatch rows in `scoped_global_key_registration_contracts` pin healthy
and failing key callbacks, the first focus failure and subsequent delivery.

Focus closure detaches the complete node tree and withdraws node properties
before callbacks can inspect or reenter it. The first callback or retirement
failure remains authoritative; later opaque owners are retained. Ordinary
notifications retain their established delivery contract, while terminal
notifications stop after failure or during an outer unwind. Key dispatch uses
registry identities and weak snapshots, so removing a still-owned handler
prevents its invocation in the current turn. The protected current callback is
retired separately after invocation. The existing
`focus_failure_and_reentrancy_matrix` covers committed terminal state, independent
capture retirement, active unwind, reentrant close during key delivery, removed
live handlers and subsequent healthy delivery. Its hostile competition cases run
in bounded child processes. An aggregate user capture whose own destructors
double-panic before returning to the containment boundary remains outside this
guarantee.

Retention keeps only what dropping would destroy. A callback, handler or
policy snapshot that is not the last `Rc` owner is released after a caught
failure, so its captures still die with the manager, node or scope that
registered them (`caught_callback_failures_leave_captures_with_their_owner`).

Closed-manager rejection uses the same ownership fence for incoming callbacks,
contexts, rectangle providers and traversal policies. Healthy rejected values
retire outside borrows; an existing unwind retains opaque incoming ownership.
Rejection cannot reinstate a terminal owner. The row
`closed_rejections_preserve_outer_failure_and_healthy_retirement` covers all
seven rejection paths, healthy destructor reentry, ordinary retirement failure
and subsequent independent-owner key delivery.

## Text-input retirement and deferred delivery

Text-input ownership, session admission and IME enablement commit before outgoing
store or callback ownership retires, without a session borrow. Deferred grants
keep accepted delivery debt separate from hook availability; a prior accepted
tail precedes newer work, and each grant rechecks the current commit gate and
lifecycle. Closure cancels its remaining tail. Detach, replacement, close,
deferred grants and owner drop share one retirement path: it preserves the
first failure, and after it, or while the thread is already unwinding, retains
the remaining stores and callbacks instead of starting another destructor
(ADR-0127). Owner drop follows the same order and retains the failure it
cannot propagate. The public `text_input_retirement_allows_reentry_and_preserves_recovery`
table pins replacement, owner release, custom store destructor competition,
grant ordering, gate changes and the next operation;
`text_input_owners_are_retained_after_a_failure_and_during_unwind` runs a
store whose destructor panics twice after a failed callback and under an
unwinding owner drop, each in its own process.

Allocating a `ClientToken` reserves identity; committing an active client admits
the replacement. Supersession follows the last successful admission, so failed
nested gate installation does not reject a valid outer client. The same public
retirement table covers this failure alone, competing outgoing retirement and
later healthy IME delivery. Successful nested replacement still supersedes its
outer operation even if subsequently detached.

### Presentation-scoped terminal withdrawal

Closing a presentation closes its focus, text-input, gesture, mouse-tracker and
interaction-lane owners in the close mode the runtime passes down
([ADR-0123](../../../docs/adr/ADR-0123-exceptional-presentation-close.md)). A
closed owner admits no new work, and a cached route that spans presentations
skips the closed one's targets while a sibling's targets stay callable. Each
owner removes its callbacks from its locks before dropping them. In preserving
mode, the callbacks still owned are retained rather than dropped
([ADR-0127](../../../docs/adr/ADR-0127-exceptional-path-retention.md)); a healthy
close, and a later call that a closed owner rejects, drop them normally.

`GestureBinding` closes in two steps. Closing sets a flag; the state each
service retires leaves it through `drain(&self)` (`RefCell::take`) only once no
dispatch of that binding is on the stack — at once when it is idle, otherwise
when the outermost dispatch returns. A close a callback requests withdraws the
arena's and the pointer router's authority immediately, so the rest of that
dispatch reaches no further handler, and defers member rejection, route release
and capture loss to the drain. Callbacks the drain runs execute with every
service borrow released; input they dispatch is refused and a close they request
is settled after the drain (`a_closed_binding_drains_services_after_dispatch`).

## Terminal drag ownership

The drag builder owns independently configured callbacks and the start strategy
until construction. Built callbacks are immutable. Last-owner destruction
withdraws contact state before retiring outgoing captures outside borrows.
Callback invocation protects its owner across reentrant cancellation or owner
release and incoming unwind.
Rejection withdraws the exact arena entry and local contact before diagnostic
subscribers run. Terminal tracking withdraws local state before arena sweep; a
reentrant same-pointer contact belongs to its new generation and is not cleared
by the old operation's tail. `drag_callback_ownership_and_retirement` exercises
these boundaries, first-failure competition and the next public drag.
