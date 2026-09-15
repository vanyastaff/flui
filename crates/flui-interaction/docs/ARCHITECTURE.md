# Architecture: flui-interaction

Crate-level design notes for `flui_interaction`. Per the [`docs/PORT.md`](../docs/PORT.md) per-crate `ARCHITECTURE.md` template (line 770) and the workspace `## Thread safety` precedent at this crate (line 778).

## Flutter source mapping

This crate ports the gesture + event subsystem of Flutter
(`packages/flutter/lib/src/gestures/`). The mapping is a near 1:1 file
correspondence; the renamed `arena/` directory bundles the formerly
crate-root `team.rs` and `signal_resolver.rs` (the back-compat shims at
`crate::team` / `crate::signal_resolver` re-export them).

| Dart (`flutter/lib/src/gestures/`)              | Rust (`crates/flui-interaction/src/`) |
|-------------------------------------------------|--------------------------------------|
| `arena.dart` (GestureArenaManager)              | `arena/mod.rs`                       |
| `team.dart`                                     | `arena/team.rs`                      |
| `events.dart` (PointerEvents re-export)         | `events.rs`                          |
| `tap.dart`                                      | `recognizers/tap.rs`                 |
| `double_tap.dart`                               | `recognizers/double_tap.rs`          |
| `long_press.dart`                               | `recognizers/long_press.rs`          |
| `drag.dart`, `horizontal_drag.dart`, `vertical_drag.dart`, `panning.dart` | `recognizers/drag.rs`, `recognizers/drag_variants.rs` |
| `scale.dart`                                    | `recognizers/scale.rs`               |
| `force_press.dart`                              | `recognizers/force_press.rs`         |
| `multi_tap.dart`                                | `recognizers/multi_tap.rs`           |
| `eager.dart`                                    | `recognizers/eager.rs`               |
| `multidrag.dart`                                | `recognizers/multidrag.rs`           |
| `tap_and_drag.dart`                             | `recognizers/tap_and_drag.rs`        |
| `pointer_router.dart`, `event_router.dart`      | `routing/`                           |
| `focus_manager.dart`                            | `routing/focus.rs`                   |
| `mouse_tracker.dart`                            | `routing/mouse_tracker.rs`           |
| `binding.dart` (GestureBinding)                 | `binding.rs`                         |
| `velocity_tracker.dart`                         | `processing/velocity.rs`             |
| `pointer_event_resampler.dart`                  | `processing/resampler.rs`            |
| `sampling_clock.dart`                           | `processing/sampling_clock.rs`       |
| `input_predictor.dart`                          | `processing/prediction.rs`           |
| `gesture_settings.dart`                         | `settings.rs`                        |

## Subsystems

| Subsystem | One-paragraph description |
|---|---|
| `arena` | Owner-local conflict resolution between competing recognisers. Tracks per-pointer `SmallVec<[Arc<dyn GestureArenaMember>; 4]>` (inline for ≤ 4 members), exact generational slots, held generations detached across pointer-ID reuse, and a lifecycle (Open → Held → Closed → Resolved). Eager acceptors win when the arena closes; teams enable multi-winner resolution. |
| `recognizers` | The 11+ recogniser types. Each implements `GestureRecognizer` (the `add_pointer` / `handle_event` / `dispose` lifecycle) and gets `GestureArenaMember` for free via the `CustomGestureRecognizer` blanket impl. State machines are kept inline per file (TapState, LongPressPhase, etc.) — no shared trait-object dispatch. |
| `processing` | Per-pointer derived data: `VelocityTracker` (LSQ fit on 20-sample circular buffer, 100 ms horizon, 40 ms stationary gate), `PointerEventResampler` (frame-rate adaptation with 100-event cap and 1 ms minimum sample interval), `InputPredictor` (kalman-style pointer extrapolation), `RawInputHandler` (low-level stream adapter), and the shared `lsq_solver` + `sampling_clock` helpers. |
| `routing` | Event dispatch infrastructure: `EventRouter`, `PointerRouter`, owner-thread TLS `FocusManager`, `FocusScopeNode` / reading-order Tab traversal, `MouseTracker` (enter/exit/hover), hit testing, and the `TransformGuard` stack-RAII for the transform stack. Off the per-pointer hot path. |
| `binding` | `GestureBinding` — owner-local glue that hosts the arena, resolves and retains the Down hit route, coalesces/resamples Moves, and runs route → arena lifecycle ordering. Contact generations prevent frame-delayed samples from crossing a reused platform pointer ID. |
| `observability` | Observability substrate. `GestureEvent` is a typed `Display` enum of recogniser / arena event names; `SPAN_RECOGNIZER` and `SPAN_ARENA` are span-name constants; `pointer_event_kind` summarises a `PointerEvent` to a short string for span fields. `#[tracing::instrument]` is applied on `RecognizerBase` start_tracking / stop_tracking and on every public `GestureArena` method. |

## Ownership and synchronization

The synchronous pointer pipeline belongs to one `UiRealm`. `GestureBinding`,
`GestureArena`, recognizers, pointer routes, and executable callbacks are
intentionally `!Send + !Sync`; callbacks may capture `Rc` widget state.
`Arc` in recognizer and arena internals provides stable identity, not a
cross-thread execution contract.

The data plane is separate. Pointer events, hit paths, IDs, transforms, and
opaque route targets remain `Send + Sync` where the renderer or embedder needs
them. Compile-time assertions in `src/lib.rs` cover only those data types.

| Site | Primitive | Reason |
|---|---|---|
| Arena slots | keyed maps + per-slot `parking_lot::Mutex` | exact slot transactions and callback-free state mutation; callbacks run after unlocking |
| Recognizer state | small `parking_lot::Mutex` fields | interior mutation behind stable `Arc` identity on the owner lane |
| Pointer router / interaction lane | `Rc` + `RefCell` | explicitly owner-local executable callbacks |
| Pointer resampler | `Arc<parking_lot::Mutex<ResamplerInner>>` | bounded data queue; sampling materializes a batch and unlocks before dispatch |
| Focus manager | transitional owner-thread state | scheduled to move from ambient TLS into presentation ownership |

There is no `unsafe impl Send/Sync` in this crate. The sealed extension traits
preserve lifecycle invariants, while negative compile-time assertions prevent
the executable gesture graph from accidentally becoming cross-thread.

## Mapping decisions

Where the Rust shape diverges from the Dart shape and why. Each entry
names the conflict, the choice, and the reference (a strategy clause,
a refusal trigger, or a precedent plan).

- **Recogniser duplication-via-`Arc` vs Dart's `ChangeNotifier` mixin.** Flutter's `TapGestureRecognizer extends ChangeNotifier` — a single class is the recogniser, the listener hub, and the lifecycle owner. In Rust the recogniser is a `Clone` struct and the lifecycle is on `RecognizerBase` (so multiple consumers can hold `Arc<Self>` cheaply). The trade-off: Dart users mutate recogniser fields directly; Rust users get a stable struct API but cannot observe field changes without an explicit notifier (deferred — Flutter's `ChangeNotifier` is in `flui-foundation::Notifier`).
- **Pointer event types are W3C `ui-events`, not a local re-implementation.** The Dart `pointer_event.dart` types are mirrored by `ui_events::pointer::*` (W3C-compliant). The mapping is type-to-type with a `DeviceId = i32` shim at the `InputEvent` enum layer. Reduces divergence from the platform layer's event types and is sanctioned by [`docs/PORT.md` ecosystem table](../docs/PORT.md) (line 700).
- **`TapButton` enum vs Dart's `kPrimaryButton` constants.** Flutter has `kPrimaryButton` / `kSecondaryButton` / `kTertiaryButton` as top-level `int` constants. Rust uses a typed `TapButton` enum (`src/recognizers/tap.rs:51-59`) with explicit `from_pointer_button` mapping — type-system enforcement vs runtime constants. The trade-off: the `TapButton` enum is `#[non_exhaustive]` so a future fourth button slot can be added without breaking downstream.
- **`ArenaEntryData` is `pub(crate)` struct vs Dart's `_GestureArenaEntry` private class.** Flutter keeps the per-pointer state as private fields on `_GestureArenaManager`; Rust uses a `pub(crate)` `SmallVec<[Arc<dyn GestureArenaMember>; 4]>` to keep the hot path alloc-free for ≤ 4 members (the typical tap + drag + long-press + double-tap case). The inline-4 capacity is justified by the bench: the `add_busy` case in `benches/gesture_arena_bench.rs` measures the heap-fallback cost separately.
- **Sealed traits vs Dart's `implements` mixin.** `GestureArenaMember` and `HitTestable` are sealed (supertrait `sealed::Sealed`). The blanket impl via `CustomGestureRecognizer` / `CustomHitTestable` is the only sanctioned extension point. The rationale is the same as the flui-foundation `sealed::Sealed` precedent: API evolution without breaking changes.
- **`pending_up` deferral for `on_tap_up`.** Before the fix, `handle_tap_up` fired `on_tap_up` and `on_tap` unconditionally on pointer up, even though every arena member receives Up events. The fix stores a `pending_up` until `accept_gesture` confirms arena victory; only the eventual winner fires the user callback. The same pattern was extended to per-button slots.
- **`try_fire_timer` runs under `gesture_state` lock.** Pre-fix `did_exceed_deadline` resolved Accepted without firing the long-press start callback. The fix calls `try_fire_timer` (which acquires `gesture_state`) before resolving. The `try_fire_timer_is_idempotent` unit test in `recognizers/long_press.rs` guards against double-fire if the timer fires twice.
- **Focus scope identity is explicit.** A `FocusScopeNode` owns an inner `FocusNode`, and that backing node carries a `Weak<FocusScopeNode>` owner link. This keeps enclosing-scope lookup, focused-child history, and `FocusManager::focus_next` / `focus_previous` rooted in the same tree instead of relying on a parallel manager structure. `descendants_are_focusable=false` gates descendant requests; a true-to-false transition evicts focus held by the node or its subtree while leaving the node eligible for a later explicit request. FLUI clears primary focus to `None` rather than selecting Flutter's previously focused child.
- **`processing::lsq_solver` is crate-internal.** Shared by `VelocityTracker` and `PointerEventResampler`; both were duplicating the matrix setup.
- **Observability is crate-public.** `pub mod observability` exports `GestureEvent`, `SPAN_RECOGNIZER`, `SPAN_ARENA`, `pointer_event_kind`. Downstream `flui-app` configures the subscriber and surfaces these to the devtools; the recognisers / arena emit them unconditionally.
- **Synchronous FIFO reentrant-focus queue vs six different reference shapes, none of them adopted as-is.** Surveyed in [`.rust-studio/specs/1040-ordered-focus-notifications/survey.md`](../../../.rust-studio/specs/1040-ordered-focus-notifications/survey.md) (issue #1040). Six engines/frameworks solve "a focus listener changes focus again" six different ways:
  - **Flutter** (`focus_manager.dart`, `FocusManager.applyFocusChangesIfNeeded` / `_markedForFocus` / `_markNextFocus`): defers every `requestFocus`/`unfocus` to a microtask (`_markNeedsUpdate` → `scheduleMicrotask`) and collapses every request made before that microtask runs into one `_markedForFocus` slot — last request wins, and `_markNextFocus` clears the pending mark outright when a request names the already-current node. Chronological by construction and every listener sees a coherent order, at the cost of up to one frame of lag and a reentrant chain that livelocks across microtasks (still yielding frames, so it never hangs the caller). The deferral traces to flutter/flutter#9074 (2017): *"Focus notifications trigger[ed] by tree mutations are now delayed by one frame, which is necessary to handle certain complex tree mutations"* — reentrancy safety is a side effect of that motive, not the reason it was built.
  - **Jetpack Compose** (`androidx.compose.ui.focus.FocusTransactionManager`): a reentrant `requestFocus` from inside `onFocusChanged` **cancels and reverts** the in-flight transaction before starting the new one. Compose later had to add a dedicated cancellation-notification path (b/319817633, b/312524818) specifically because a silent revert left observers holding stale state — a second notification kind existing only to patch the first one's blind spot.
  - **WHATWG HTML**: browsers apply a nested `focus()` call synchronously and dispatch its `blur`/`focus` events at commit time (chronological by construction, like Flutter, but with no lag). The spec's own "locked for focus" reentrancy guard was never implemented by any engine and was formally removed (whatwg/html#11182, 2025) — the only bound on a reentrant ping-pong is the engine's stack-recursion limit.
  - **GPUI** (`zed-industries/zed`, `crates/gpui/src/window.rs`, `Window::focus` / `Window::draw`): `focus` only records intent (a target handle plus a `focus_generation` bump); the actual focus-changed event is computed at draw time, as a diff between the previous and current rendered frame's focus path. A listener that moves focus during that diff is handled by deferring to the *next* frame rather than recursing. Their own source comment records exactly the incident this crate's drain budget exists to prevent: *"scheduling a redraw below would dispatch focus-lost again, looping forever"* — they fixed it by narrowing what counts as listener-caused movement, not by a budget.
  - **Masonry/Xilem** (`masonry_core/src/passes/update.rs`, `run_update_focus_pass`): `request_focus` writes a pending `next_focused_widget` slot; the update pass commits it and delivers `FocusChanged` to the old and new widget — last-wins, applied in a pass after event handling, Flutter's family of solutions.
  - **Qt** (widgets): fully synchronous nested dispatch — `setFocus` called from inside `focusInEvent` simply recurses, and `QApplication::focusChanged` fires per change with no engine-side reentrancy bound at all; the escape hatch it gives a handler is `QFocusEvent::reason()`, so code can recognize and decline to react to a focus change it caused itself. Real infinite-focus-loop reports exist in the wild for exactly this shape.

  FLUI's `request_focus`/`unfocus` are synchronous and must return a decision immediately (an existing, unrelated contract), so Flutter's "defer to a microtask that doesn't exist yet at this point in a synchronous call stack" isn't available. `FocusManager` instead queues a reentrant request (`pending_focus_transitions: RefCell<VecDeque<Option<Rc<FocusNode>>>>`, `None` = unfocus) and applies every queued request FIFO once the in-flight notification (`notification_depth: Cell<u32>`) returns to zero — each with its own commit, node-listener pass, and manager-listener publication, in request order. This reproduces Flutter's chronological guarantee ("a reentrant request becomes the next transition, never an earlier one republished late") without the up-to-one-frame lag, and without Compose's revert-plus-cancellation-event surface (a synchronous manager has no torn-down transaction to revert — every accepted request either commits or is still queued, never both, so there is nothing a cancellation event would need to announce) and without the browsers' unbounded, uncontrolled recursion. `finish_node_replacement` and `close` publish under the same guard: a node replacement's own outer edge fires only when primary identity actually changed (a replacement that only changes a focused node's ancestry publishes no manager-level edge — this deleted the pre-fix `focus_is_unchanged`/`latest` check, which was reentrancy-safe only for that one path and which also fired a same-identity edge on every ancestry-only change; see the reworked `replacing_an_attached_ancestor_preserves_descendant_attachment_and_focus` test), and `close`'s own final notification is skipped rather than published out of order when `close` itself runs reentrantly. A reentrant chain with no natural end (two listeners that keep re-requesting each other) has no analogue among the six references that FLUI can lean on as-is — Flutter yields a frame per bounce, browsers hit a recursion error, Compose reverts, GPUI shipped exactly this as a production "looping forever" bug and fixed it by narrowing the trigger rather than bounding it, Masonry/Xilem's own tracker (linebender/xilem#390) flags the inherited design as needing simplification without a reentrancy fix, and Qt hands the handler `QFocusEvent::reason()` instead of a bound — a synchronous Rust call stack has none of those outs, so the drain is bounded at `FocusManager::REENTRANT_FOCUS_DRAIN_BUDGET` (32) applications per outermost call and drops the remainder with one latched `tracing::warn!` naming the last-requested node id and the count of dropped requests (`ping_pong_listeners_are_bounded_and_warned`). Listener dispatch also matches Flutter's `_HighlightModeManager.notifyListeners` (`if (_listeners.contains(listener))`): both `FocusManager::notify_listeners` and `FocusNode::notify_listeners_after_tree_change` re-check that a listener is still registered immediately before calling it, so one listener removing another (or itself) mid-dispatch is never called again in that same dispatch (`listener_removed_during_dispatch_is_not_called`, `node_listener_removed_during_dispatch_is_not_called`).

## Testing strategy

| Command | Purpose |
|---|---|
| `cargo test -p flui-interaction --all-features` | Unit, integration, feature-gated testing helpers, and doctests across arena / recognisers / processing / routing / timer. |
| `cargo test --doc -p flui-interaction` | Runnable public examples; illustrative `rust,ignore` snippets stay excluded until their surrounding framework fixtures exist. |
| `cargo bench -p flui-interaction` | 4 Criterion benches. All use `black_box` on inputs and outputs. Hot-path regression guards; baseline numbers to be captured in the next release. |
| `cargo clippy -p flui-interaction --lib --tests --benches -- -D warnings` | Lint gate — zero warnings. |
| `cargo fmt -p flui-interaction --check` | Format gate. |

## Observability

The observability substrate lives at [`crate::observability`](src/observability.rs) (re-exported at
the crate root as `flui_interaction::observability::*` and the three
`GestureEvent` / `SPAN_RECOGNIZER` / `SPAN_ARENA` / `pointer_event_kind`
items). The hot paths (`RecognizerBase::start_tracking`,
`RecognizerBase::stop_tracking`, `GestureArena::add` / `close` /
`resolve` / `sweep`) carry `#[tracing::instrument]` with a typed
`event = %GestureEvent::*` span field. Configure your subscriber at
the app boundary; the crate does not install one. Filter via
`RUST_LOG=flui_interaction::arena=debug,flui_interaction::recognizers=trace`.

## Friction log

- **`docs/ARCHITECTURE.md` (this file) is the template-driven version;** the pre-template `crates/flui-interaction/docs/ARCHITECTURE.md` body (gesture state-machine diagrams, hit testing walk) lives as a companion. Per [`docs/PORT.md` line 798](../docs/PORT.md), relocation to crate root is deferred to the doc-tidying PR.
- **`is_resolved(pointer)` returns `bool` not `Result`.** Arena resolution can't fail in this design (the worst case is a `parking_lot::Mutex` poison — the `Deref` impl swallows it for ergonomics, and the arena entry is dropped). If you need poison-detection, wrap the call site in `catch_unwind` rather than changing the API.
- **`make_*_event` test helpers are `#[cfg(any(test, feature = "testing"))]`.** The benches depend on the `testing` feature being enabled in `dev-dependencies`. Documented at `Cargo.toml`; the gates will surface any missing opt-in.

## Outstanding refactors

- **Doc-test sweep: convert the remaining 72 `rust,ignore` to runnable.** The `processing::InputPredictor` and `routing::FocusManager` doc-tests are the next highest-value targets. The `testing` module builders (`ModifiersBuilder`, `KeyEventBuilder`) are the third tier. Land as a follow-up PR.
- **Property tests for the gesture arena** (deferred). `proptest` over a sequence of `add` / `close` / `sweep` operations, asserting: every reachable pointer has a state, no arena has two winners, `is_resolved` ⇔ `winner_count >= 1` after `close`. Bench time + property-cost justifies a separate `flui-interaction/tests/proptest_arena.rs` file.
- **Loom test coverage for the arena's DashMap + Mutex pairing** (deferred). `loom` over a small parallel `add` / `resolve` workload. Same precedent as `flui-rendering` — needs a `#[cfg(loom)]` gate.
- **Bench fidelity pass: realistic workloads.** Current benches use synthetic events; the next pass should replay recorded gesture traces from `flui-app` (TBD where they live). The `testing::recording` module is the substrate.
- **Re-export the `pub mod observability` at `crate::prelude`** once the devtools substrate is stable — currently only the `GestureEvent` / `SPAN_*` items are re-exported at the crate root.

## Index of in-crate companion documents

These live alongside this templated `ARCHITECTURE.md` and are
referenced from it. They predate the template and remain as
subsystem-level deep-dives (per [`docs/PORT.md` line 134](../docs/PORT.md)):

- [`docs/GESTURES.md`](docs/GESTURES.md) — gesture catalogue.
- [`docs/HIT_TESTING.md`](docs/HIT_TESTING.md) — hit-test walk.
- [`docs/INTEGRATION.md`](docs/INTEGRATION.md) — `GestureBinding`
  integration guide for downstream crates.
- [`docs/PERFORMANCE.md`](docs/PERFORMANCE.md) — performance notes
  (60 fps / 16 ms / 0 alloc on hot path).
