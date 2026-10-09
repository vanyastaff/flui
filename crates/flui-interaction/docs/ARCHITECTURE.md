# Architecture: flui-interaction

Crate-level design notes for `flui_interaction`: subsystems, ownership, mapping decisions, thread safety, friction and outstanding refactors.

## Subsystems

| Subsystem | One-paragraph description |
|---|---|
| `arena` | Owner-local conflict resolution between competing recognizers. Per-pointer members are `Weak<dyn GestureArenaMember>` in an inline-four `SmallVec`. Generational slots keep held competitions separate across pointer-ID reuse. Eager acceptors win when the arena closes; each notification upgrades its weak participant immediately before invoking it. |
| `recognizers` | Ten recognizer types (tap, double tap, long press, drag, scale, force press, multi-tap, multi-drag, eager, tap-and-drag) and drag-axis builders. Each implements the open `GestureRecognizer` and `GestureArenaMember` traits. Builders configure immutable callbacks before returning `Rc` ownership. `ArenaMembership` names an exact weak allocation; `PrimaryContact` owns one admitted sequence, its settings snapshot, identity and deadline. `RecognizerSet` shares ordered weak attachments with listeners. |
| `processing` | Per-pointer derived data: `VelocityTracker` (LSQ fit on 20-sample circular buffer, 100 ms horizon, 40 ms stationary gate), `PointerEventResampler` (frame-rate adaptation with 100-event cap and 1 ms minimum sample interval), standalone OneEuro filters, the crate-internal `lsq_solver` (used by `VelocityTracker` only) and `sampling_clock`. |
| `routing` | Event dispatch infrastructure: `PointerRouter`, the presentation-owned `FocusManager` (`FocusManager::new` returns an `Rc<Self>`; there is no thread-local focus state), `FocusScopeNode` / reading-order Tab traversal, `MouseTracker` (enter/exit/hover), hit testing, the `InteractionLane` that resolves and invokes pointer routes, and the `TransformGuard` stack-RAII for the transform stack. Route resolution on Down and cached-route invocation on every Move/Up are on the per-pointer hot path (`benches/pointer_route_bench.rs`). |
| `binding` | `GestureBinding` — owner-local glue that hosts the arena, resolves and retains the Down hit route, coalesces/resamples Moves, and runs route → arena lifecycle ordering. Contact generations prevent frame-delayed samples from crossing a reused platform pointer ID. |
| `observability` | `GestureEvent` gives typed event names; `SPAN_RECOGNIZER` and `SPAN_ARENA` name spans, and `pointer_event_kind` describes a pointer event for tracing. Rejection and terminal tracking commit local withdrawal before diagnostics, because subscribers can reenter or panic. The app installs the subscriber. |

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
| Pointer resampler | `Arc<parking_lot::Mutex<ResamplerInner>>` | bounded data queue; sampling materializes a batch and unlocks before dispatch |
| Focus manager | `Rc` + `RefCell` / `Cell`, one per presentation | owner-local focus tree and listeners |

Negative compiler fixtures reject transfer of executable recognizers, builders
and attachments to another thread. Gesture extension traits are open; their
implementations cannot change arena ordering or bypass exact membership checks.

## Mapping decisions

Local design choices and why. Each entry names the conflict, the choice, and the reference (a strategy clause, a design rule, or a precedent plan).

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
  TapAndDrag checks initial and incremental displacement before publication;
  overflow cancels the attempt rather than fabricating a clamped delta.
  `tap_and_drag_resolves_through_the_shared_arena` covers both boundaries,
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
  refresh in identity order, each failed hit test preserves that device's prior
  state for retry, and every committed callback batch runs before the first
  failure resumes. Replaced and departed captures retire outside the tracker
  borrow under ADR-0127. `shared_region_exit_per_device`,
  `ambient_refresh_contains_each_device` and
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
- **`TapButton` is a typed enum, not integer button constants.** `TapButton` (`src/recognizers/tap.rs`) maps pointer buttons explicitly through `from_pointer_button`, so the type system enforces the choice. It is `#[non_exhaustive]` so a future fourth button slot can be added without breaking downstream.
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
- **`processing::lsq_solver` is crate-internal.** `VelocityTracker` is its only user; the resampler interpolates linearly and does not fit a polynomial.
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

## Observability

The observability substrate lives at [`crate::observability`](../src/observability.rs) (re-exported at
the crate root as `flui_interaction::observability::*` and the three
`GestureEvent` / `SPAN_RECOGNIZER` / `SPAN_ARENA` / `pointer_event_kind`
items). Arena admission and resolution emit tracing spans and typed gesture
events. Configure your subscriber at
the app boundary; the crate does not install one. Filter via
`RUST_LOG=flui_interaction::arena=debug,flui_interaction::recognizers=trace`.

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
- **Bench fidelity pass: realistic workloads.** Current benches use synthetic events; recorded traces would need a recording format, which this crate does not have (`testing` holds only the `input` event builders).
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
