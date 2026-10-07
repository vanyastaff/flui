# Architecture: flui-interaction

Crate-level design notes for `flui_interaction`: subsystems, ownership, mapping decisions, thread safety, friction and outstanding refactors.

## Subsystems

| Subsystem | One-paragraph description |
|---|---|
| `arena` | Owner-local conflict resolution between competing recognizers. Per-pointer members are `Weak<dyn GestureArenaMember>` in an inline-four `SmallVec`. Generational slots keep held competitions separate across pointer-ID reuse. Eager acceptors win when the arena closes; each notification upgrades its weak participant immediately before invoking it. |
| `recognizers` | Ten recognizer types (tap, double tap, long press, drag, scale, force press, multi-tap, multi-drag, eager, tap-and-drag) and drag-axis builders. Each implements the open `GestureRecognizer` and `GestureArenaMember` traits. Builders configure immutable callbacks before returning `Rc` ownership. `ArenaMembership` names an exact weak allocation; `PrimaryContact` owns one admitted sequence, its settings snapshot, identity and deadline. `RecognizerSet` shares ordered weak attachments with listeners. |
| `processing` | Per-pointer derived data: `VelocityTracker` (LSQ fit on 20-sample circular buffer, 100 ms horizon, 40 ms stationary gate), `PointerEventResampler` (frame-rate adaptation with 100-event cap and 1 ms minimum sample interval), `InputPredictor` (velocity extrapolation with optional acceleration and prediction smoothing), `RawInputHandler` (low-level stream adapter), the crate-internal `lsq_solver` (used by `VelocityTracker` only) and `sampling_clock`. |
| `routing` | Event dispatch infrastructure: `EventRouter`, `PointerRouter`, the presentation-owned `FocusManager` (`FocusManager::new` returns an `Rc<Self>`; there is no thread-local focus state), `FocusScopeNode` / reading-order Tab traversal, `MouseTracker` (enter/exit/hover), hit testing, the `InteractionLane` that resolves and invokes pointer routes, and the `TransformGuard` stack-RAII for the transform stack. Route resolution on Down and cached-route invocation on every Move/Up are on the per-pointer hot path (`benches/pointer_route_bench.rs`). |
| `binding` | `GestureBinding` — owner-local glue that hosts the arena, resolves and retains the Down hit route, coalesces/resamples Moves, and runs route → arena lifecycle ordering. Contact generations prevent frame-delayed samples from crossing a reused platform pointer ID. |
| `observability` | `GestureEvent` gives typed event names; `SPAN_RECOGNIZER` and `SPAN_ARENA` name spans, and `pointer_event_kind` describes a pointer event for tracing. Rejection and terminal tracking commit local withdrawal before diagnostics, because subscribers can reenter or panic. The app installs the subscriber. |

## Ownership and synchronization

The synchronous pointer pipeline belongs to one `UiRealm`. `GestureBinding`,
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
- **Pointer event types are W3C `ui-events`, not a local re-implementation.** Pointer events are `ui_events::pointer::*` (W3C-compliant), with a `DeviceId = i32` shim at the `InputEvent` enum layer. This keeps the crate aligned with the platform layer's event types and follows the workspace preference for a mature crate over a hand-rolled one.
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
| `cargo test --doc -p flui-interaction` | Public source-documentation examples; standalone Markdown snippets also need inclusion in a rustdoc test target to run. |
| `cargo bench -p flui-interaction` | Criterion workloads and saved-baseline comparisons (see [PERFORMANCE.md](PERFORMANCE.md)). |
| `cargo clippy -p flui-interaction --lib --tests --benches -- -D warnings` | Lint gate — zero warnings. |
| `cargo fmt -p flui-interaction --check` | Format gate. |

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
  its containing document is included by a rustdoc target.
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
