# ADR-0018: `FutureBuilder` / `StreamBuilder` need **two seams before any widget code** — a `'static` rebuild handle reachable from `ViewState`, and a frame-driven async driver — and Rust's move-only futures force one unavoidable API divergence: **identity becomes an explicit key**

*Flutter's `FutureBuilder` works because a Dart `Future` is a cheap, identity-comparable object that completes on the UI isolate's event loop, and because `State.setState` can be called from any callback. FLUI had none of those three when this ADR was written. Nothing in `flui-view` handed a `ViewState` a `'static` rebuild handle (`BuildContext::mark_needs_build` is borrow-scoped, and `BuildContext::owner()` returned `None` — U1 replaced it); no crate in the view/widget/app stack owned an async runtime; and a Rust `Future` is move-only, not `Clone`, not `Eq`, so `oldWidget.future != widget.future` cannot be written. This ADR designs the seams, pins the snapshot state machine against the reference, records the divergences, and tracks the landed enabling slices.*

---

- **Status:** Accepted — **U1–U6 landed 2026-07-09.** `FutureBuilder` and `StreamBuilder` are public from `flui-widgets` + prelude; parity re-checked against `.flutter/` (see *Parity findings (U6)*); the keyed-identity divergence is signed off below. ADR complete.
- **Date:** 2026-07-09
- **Deciders:** chief-architect; consult view owner (the rebuild-handle seam + its refusal trigger), scheduler owner (frame-driven driver, phase placement), repository owner (the keyed-identity divergence — this is a public API shape we cannot walk back), qa-lead (headless determinism: completing a future inside a test frame)
- **Relates to:** reuses the `ExternalBuildScheduler` channel that already backs `AnimatedView` (`owner/build_owner.rs`), and the shared-cell pattern of [`ADR-0017`](ADR-0017-build-during-layout-callback-seam.md) U1 (`LayoutConstraintsCell`). Constrained by [`FOUNDATIONS.md`](../FOUNDATIONS.md): the catalog crates must not depend on `flui-reactivity`, and "an application-author signal crate that drives `Element::mark_needs_build` from outside the catalog is a permitted post-parity opt-in, **gated by a refusal trigger barring signal subscriptions from `build`/`layout`/`paint`**".
- **Blocks:** `FutureBuilder` / `StreamBuilder` (tracker B1.1).
- **Gate:** ARCH-GATE (this doc) → then DEV-GATE per slice.

---

## Reference

Cross-checked against `.flutter/` (Flutter master `3.33.0-0.0.pre-6280-g88e87cd963f`):

- `packages/flutter/lib/src/widgets/async.dart` — `ConnectionState`, `AsyncSnapshot`,
  `StreamBuilderBase` + `_StreamBuilderBaseState`, `StreamBuilder`, `FutureBuilder` +
  `_FutureBuilderState`.
- `packages/flutter/test/widgets/async_test.dart` — the behavioral oracles.

Everything stated below about Flutter is read from those files. `FutureBuilder` and `StreamBuilder` are now public from `flui-widgets` after U6; the widget-level parity findings and documented divergences are recorded below. The state tables are the specification U3 satisfies, and the tests in *Required tests* are transcriptions of Flutter's own.

---

## Context: three things FLUI does not have

### 1. No `'static` rebuild handle for a `ViewState`

Flutter's completion callbacks call `setState`, which reaches `Element.markNeedsBuild`
through the `State._element` backreference. In FLUI:

- `BuildContext::mark_needs_build(&self)` is borrow-scoped — it cannot be captured
  by a `'static + Send` callback.
- `BuildContext::owner()` **was** a stub that unconditionally returned `None`
  (`context/element_build_context.rs`). U1 deleted it and put `rebuild_handle()` in
  its place, so there is no second fake owner path.
- `ViewState::init_state(&mut self, ctx: &dyn BuildContext)` receives only that
  context; `did_update_view(&mut self, old, new)` and `dispose(&mut self)` receive
  **no context at all**.

The machinery *exists*, one layer down and out of reach: `ElementCore::create_mark_dirty_callback()`
returns an `Arc<dyn Fn() + Send + Sync>` that sets the element's dirty flag and pushes
its `ElementId` into `BuildOwner::external_inbox` via `ExternalBuildScheduler`, which
`build_scope` drains at frame start and which requests a frame from the platform.
That is exactly what `AnimatedView` uses. `StatefulView` has no equivalent hook.

### 2. No async runtime in the view/widget/app stack

`tokio` is a workspace dependency, but only `flui-assets` and `flui-cli` use it.
`flui-view`, `flui-widgets`, and `flui-app` declare no `tokio`, no `futures`, and no
executor. `flui-app` reaches for `pollster::block_on` for GPU init and nothing else.

So there is no answer today to "who polls the future". `flui-scheduler` does have the
shape of one: a `SchedulerPhase` mirroring Flutter's (`Idle`, `TransientCallbacks`,
**`MidFrameMicrotasks`**, `PersistentCallbacks`, `PostFrameCallbacks`), a microtask
queue drained by `flush_microtasks()` inside the frame, a priority `TaskQueue`, and
`request_frame()` / `set_on_frame_scheduled`.

### 3. A Rust `Future` is move-only, not `Clone`, not `Eq`

`_FutureBuilderState.didUpdateWidget` is built on `oldWidget.future == widget.future`
— identity comparison of a heap object that the widget holds by cheap reference and
that survives being copied into a new widget instance. In Rust:

- a `Future` cannot be stored in a `View` that must be `Clone + Send + Sync + 'static`
  (every FLUI view is cloned on rebuild);
- two futures cannot be compared;
- polling one requires unique ownership.

This is not a style problem; it is the reason the public API cannot be
`FutureBuilder::new(future)`.

---

## Decision

### D1 — Where async completion is subscribed

**In `ViewState`, not in the element or the render object.** `FutureBuilder` and
`StreamBuilder` are ordinary `StatefulView`s. Their state owns a **subscription
token**:

```text
init_state(ctx)      → capture a RebuildHandle (D2); create the snapshot from
                       initial_data; spawn the driver (D3); snapshot → Waiting
did_update_view(old,new) → if the identity key changed: drop the old token
                       (cancels), snapshot → in_state(None), spawn anew → Waiting
dispose()            → drop the token (cancels)
```

The token is `Drop`-cancelling, so there is no path where a completed future writes
into a disposed state. `dispose(&mut self)` takes no context, which is exactly why
cancellation must be carried by an owned value rather than by a call.

### D2 — How completion schedules a rebuild without violating the sync rules

**Completion never touches the element tree, the render tree, or the pipeline.** It
does exactly two things, in this order:

1. writes the new `AsyncSnapshot` into a shared cell (`Arc<Mutex<AsyncSnapshot<…>>>`),
   the direct analogue of ADR-0017's `LayoutConstraintsCell`;
2. calls `RebuildHandle::schedule()`, which is `ExternalBuildScheduler::schedule(id)` —
   insert `ElementId` into the shared inbox (a `HashSet`, so a burst of stream events
   between frames collapses to one entry) and, if newly queued, ask the platform for a
   frame.

`BuildOwner::build_scope` drains that inbox at frame start and rebuilds the element,
whose `build` reads the cell. So the rebuild happens **in the build phase of a
subsequent frame**, on the frame thread, synchronously. A wake that lands mid-frame
stays in the inbox for the next frame — the behavior already documented on
`build_scope`'s drain ("Flutter defers mid-frame schedules"). Nothing can rebuild
during layout or paint.

**The seam.** Expose the handle on `BuildContext`:

```rust
/// Cloneable, Send + Sync + 'static. Schedules this element for the next
/// `build_scope` drain and requests a frame. Obtainable ONLY from `init_state`
/// / `did_change_dependencies`; calling `schedule()` is legal from any thread.
fn rebuild_handle(&self) -> RebuildHandle;
```

`RebuildHandle` is a newtype over `(ExternalBuildScheduler, ElementId)` — both already
exist and are already `Send + Sync`. This closes the "no `'static` rebuild handle"
hole that ADR-0017 also had to route around. **Landed in U1.**

**Refusal trigger (mandatory, ships with the seam).** FOUNDATIONS permits an
out-of-catalog `mark_needs_build` driver only when "gated by a refusal trigger barring
signal subscriptions from `build`/`layout`/`paint`". `scripts/port-check.sh` must
therefore reject `rebuild_handle()` appearing in a `fn build`, `perform_layout`, or
`paint` body. Acquiring a handle during build is how you write an unbounded rebuild
loop.

### D3 — Who polls the future: a frame-driven driver, not a runtime dependency

**Recommended: a single-threaded, frame-driven driver in `flui-scheduler`.**

```rust
// flui-scheduler
impl Scheduler {
    /// Queue `fut` to be polled by the binding's mid-frame async-driver step.
    /// The waker requests a frame, so a completion from any thread wakes the UI.
    pub fn spawn_local(&self, fut: Pin<Box<dyn Future<Output = ()> + Send>>) -> TaskToken;
}
```

- Polled by the binding in Flutter's `MidFrameMicrotasks` slot: after transient/vsync
  work, before `build_scope` and render-frame work. U2 found that this must **not**
  mutate `SchedulerPhase`; the phase machine is driven elsewhere and is strictly
  forward-only. `drive_async_tasks` instead asserts it is not called during
  `PersistentCallbacks`.
- The `Waker` calls `Scheduler::request_frame()`. A completion signalled from a worker
  thread (a `tokio::spawn`'d `JoinHandle`, a `oneshot::Receiver`) wakes the UI thread.
- `TaskToken` cancels on drop → D1's subscription token is one of these.

This is **Flutter parity, not a compromise**: a Dart `Future` completes on the UI
isolate's event loop, and `FutureBuilder`'s callbacks run there. Polling on the frame
thread reproduces that. CPU-bound work is the application's problem in both
frameworks — the user hands us a future that is already offloaded.

Rejected: **give `flui-app` a `tokio` runtime and let the catalog reach it.**
It puts a runtime in the dependency graph of every widget consumer, forces a
runtime-flavor choice on app authors, and buys nothing: the completion still has to
hop to the frame thread to rebuild.

Rejected: **poll in a `PostFrameCallback`.** A future completing during a frame would
then rebuild only two frames later.

**`HeadlessBinding` must drive the same driver**, or async tests will diverge from
production the way the `flui-widgets` harness diverged in ADR-0017 U4. One shared
call site, per the discipline that ADR established.

### D4 — Snapshot state model

Ported 1:1 in *behavior*, restructured for Rust.

```rust
pub enum ConnectionState { None, Waiting, Active, Done }   // exact, 4 variants

pub struct AsyncSnapshot<T, E> {
    connection_state: ConnectionState,
    data: Option<T>,
    error: Option<E>,
}
```

**Divergences, deliberate:**

| Flutter | FLUI | Why |
|---|---|---|
| `error: Object?` + `stackTrace: StackTrace` | generic `E`, no stack trace | Rust has no ambient stack traces on error values. `E` comes from `Future<Output = Result<T, E>>` — errors are in the type, not thrown. A future that cannot fail uses `E = Infallible`. |
| `T get requireData` throws | no `require_data` | It exists in Dart because `data` is nullable and there is no `Option`. `snapshot.data()` returns `Option<&T>`; the caller uses `expect`. `PANIC-POLICY.md` reserves panics for internal invariants. |
| `AsyncSnapshot` is a value; `builder` gets it by value | `builder` gets `&AsyncSnapshot<T, E>` | Avoids `T: Clone`. FOUNDATIONS: "Application state carries **no trait bound beyond `'static`** — the Druid mistake is the one most dangerous trap." |
| `FutureBuilder.debugRethrowError` | not ported | A debug-only hook that re-throws into Dart's zone. No zone, no analogue. |

**`FutureBuilder` transitions** (`_FutureBuilderState`, verified):

| Event | Snapshot |
|---|---|
| `init_state`, `initial_data = None` | `nothing()` → `ConnectionState::None`, no data |
| `init_state`, `initial_data = Some(d)` | `with_data(None, d)` |
| after subscribe, if not already `Done` | `.in_state(Waiting)` — **data preserved** |
| future is `None` | no subscribe; stays `None`/`nothing` |
| completes with value | `with_data(Done, v)` |
| completes with error | `with_error(Done, e)` |
| `did_update_view`, same key | **no-op** (early return) |
| `did_update_view`, new key | if subscribed: unsubscribe, `.in_state(None)` (**data preserved**); subscribe → `.in_state(Waiting)`. `initial_data` is **not** re-applied — Flutter's `'ignores initialData when reconfiguring'` |
| `dispose` | unsubscribe |

The "already `Done` before `in_state(Waiting)`" guard exists for `SynchronousFuture`,
whose `.then` runs inline. Rust's equivalent is a future that is `Ready` on first poll;
the guard must be kept, and the driver must poll **once eagerly at subscribe time** for
it to be observable (Flutter's `'gives expected snapshot with SynchronousFuture'`).

**`StreamBuilder` folds** (`StreamBuilder` overrides, verified):

| Fold | Result |
|---|---|
| `initial()` | `with_data(None, d)` if `initial_data`, else `nothing()` |
| `after_connected` | `.in_state(Waiting)` |
| `after_data(_, d)` | `with_data(Active, d)` — **clears any error** |
| `after_error(_, e)` | `with_error(Active, e)` — **clears any data** |
| `after_done` | `.in_state(Done)` — preserves last data *or* error |
| `after_disconnected` | `.in_state(None)` |

`StreamBuilderBase<T, S>` (the fold-summary generalization) is **not** ported in the
first slice: it exists in Dart to let `StreamFold`-style aggregations reuse the
subscription machinery. Port it only if a second consumer appears.

### D5 — Same-future / new-future update semantics: identity becomes a key

Flutter: `if (oldWidget.future == widget.future) return;`. Rust cannot express that.

**Decision: the public constructor takes an identity key and a factory.**

```rust
// Resubscribes iff `key` changes (PartialEq). The factory runs at subscribe time.
FutureBuilder::keyed(key: K, make: impl FnOnce() -> Fut, builder: …)
StreamBuilder::keyed(key: K, make: impl FnOnce() -> St,  builder: …)
```

- `K: PartialEq + Send + Sync + 'static` — typically a request id, a URL, a `()` for
  "subscribe once".
- The factory is `FnOnce` and is *not* stored, so the view stays `Clone`.
- Same key ⇒ same subscription, exactly Flutter's early return.
- Different key ⇒ Flutter's unsubscribe → `in_state(None)` → resubscribe → `Waiting`.

This is a **necessary divergence**, and it is also the better API: Flutter's most
notorious `FutureBuilder` footgun is calling an `async` function *inside* `build`,
which creates a new `Future` every rebuild and re-enters `waiting` forever. The keyed
form makes that unrepresentable. It had to be signed off by the repository owner before
code landed — a public shape we cannot walk back.

Rejected alternatives: `Arc<Mutex<Option<BoxFuture>>>` (leaks the cell into user code);
`futures::future::Shared` (needs `T: Clone` and a `futures` dependency, and still gives
no identity across rebuilds).

### D6 — Stream subscription lifecycle and errors

- The subscription is the D3 `TaskToken` driving a loop that polls the stream and, per
  item, applies the D4 fold and calls `RebuildHandle::schedule()`.
- `Stream<Item = Result<T, E>>`. A Dart stream **continues after an error** unless
  `cancelOnError`; a Rust stream of `Result` does the same, so `after_error` → `Active`
  and polling continues. Matching behavior, no special casing.
- Stream end (`Poll::Ready(None)`) → `after_done` → `Done`, token retired.
- `dispose` / key change → drop the token → the poll loop stops. Unlike Dart, this is
  **true cancellation** of the producer, not merely ignoring late callbacks.
- **Generation guard, defense in depth.** Even with drop-cancellation, keep Flutter's
  `_activeCallbackIdentity` idea as a generation counter in the cell: a write whose
  generation is stale is discarded. Cheap, and it makes a mis-ordered wake impossible
  rather than merely unlikely.

The `Stream` trait itself is not in `std`. Take `futures-core` (trait-only, no
executor, no proc macros) as a workspace dependency, not full `futures`.

### D7 — Does FLUI need a seam before widget code? **Yes — two, and they are the work**

The widgets are thin; the seams are not. In order:

1. **S1 `RebuildHandle`** (`flui-view`) + its `port-check.sh` refusal trigger.
   Without it, no `ViewState` can be rebuilt from a completion callback. This also
   retires the `BuildContext::owner()` stub, which currently lies.
2. **S2 frame-driven driver** (`flui-scheduler`), wired identically into
   `AppBinding` and `HeadlessBinding`.

Only then are `FutureBuilder`/`StreamBuilder` ordinary `StatefulView`s of ~100 lines
each.

---

## Required tests (before any public export)

Transcribed from `.flutter/packages/flutter/test/widgets/async_test.dart`; expected
values from the reference, not from running the code.

**Seams.** S1: a handle scheduled from a worker thread rebuilds on the next frame and
never mutates the tree off-thread; a handle outliving its element is inert. The
refusal trigger fires on `rebuild_handle()` inside `build`/`perform_layout`/`paint`.
S2: a future completing between frames wakes exactly one frame; `HeadlessBinding` and
`AppBinding` drive the same driver (the ADR-0017 U1 wiring-test pair, repeated).

**`FutureBuilder`.** `tracks life-cycle of Future to success` (`None`→`Waiting`→`Done+data`);
`… to error`; `runs the builder using given initial data`;
`ignores initialData when reconfiguring`; `gracefully handles transition to/from null future`;
`gracefully handles transition to other future` (old data survives the `None`/`Waiting` hop);
`gives expected snapshot with SynchronousFuture` (an immediately-`Ready` future never shows `Waiting`);
same-key rebuild does not resubscribe; `dispose` cancels and a late completion is dropped.

**`StreamBuilder`.** `tracks events and errors of stream until completion`
(`Waiting`→`Active(d)`→`Active(err)`→`Active(d)`→`Done`); error clears data and data
clears error; `runs the builder using given initial data`; `ignores initialData when
reconfiguring`; `gracefully handles transition to/from null stream`; `… to other stream`;
`dispose` cancels the subscription and stops the producer.

---

## Consequences

**Positive.** Both seams are reuse, not invention: the rebuild channel is the one
`AnimatedView` already rides, and the snapshot cell is ADR-0017's pattern. No runtime
enters the catalog's dependency graph. The keyed API deletes Flutter's worst
`FutureBuilder` footgun by construction. `T: Clone` is never required.

**Negative.** `BuildContext` grows a method that is dangerous in the wrong phase — hence
the mandatory refusal trigger. `flui-scheduler` acquires an executor, small but real,
and both bindings must drive it identically or async tests will lie. `futures-core`
is a new workspace dependency. The `keyed` shape is a public divergence from Flutter,
and the migration cost of changing our minds later is total.

**Resolved by U3.** `AsyncSnapshot` / `ConnectionState` live in `flui-foundation`, next
to `Listenable` / `ValueNotifier`. They are pure data with no render, future, executor,
or widget dependency, and this keeps `flui-material` from re-declaring them later.

---

## Implementation sequence

- **U1 — `RebuildHandle` seam** (`flui-view`) + `port-check.sh` trigger + tests. No widget. ✅ **Landed 2026-07-09.**
  `RebuildHandle` (`Clone + Send + Sync + 'static`) wraps the existing
  `ExternalBuildScheduler` + `ElementId`; `BuildContext::rebuild_handle()` replaces the
  `owner() -> None` stub, which is deleted rather than left as a second fake path.
  `ElementCore::create_mark_dirty_callback` is now a thin wrapper over the same handle,
  so `AnimatedView` and a future async builder ride one channel. Trigger **#22**
  (`scripts/check-frame-capability-scope.sh`, a brace-depth scanner with accept/reject
  fixtures) rejects acquiring a handle in `build`/`perform_layout`/`paint`/composite
  bodies. **One latent bug found:** the `build_scope` external-inbox drain never marked
  the drained element dirty — `AnimatedView` masked it by setting the flag inside its own
  callback, so a plain `(inbox, ElementId)` handle would queue an element that
  `perform_build` then short-circuited on `!should_build()`, silently reconciling its
  children away. The drain now calls `tree.mark_needs_build(id)`; verified red-then-green.
- **U2 — frame-driven driver** (`flui-scheduler`) + identical `AppBinding`/`HeadlessBinding`
  wiring + a headless "complete this future inside a frame" test helper. No widget.
  ✅ **Landed 2026-07-09.** `AsyncDriver` + `TaskToken` + `BoxedTask`, on `std::future`
  / `std::task` only — no runtime, no `futures` dependency, and `flui-view` /
  `flui-widgets` / `flui-app` still declare none. `Scheduler::spawn_local` /
  `drive_async_tasks` are the one driver step; `HeadlessBinding::pump_frame` and
  `AppBinding::draw_frame` both call it between the vsync tick and `build_scope`, so a
  completion is observed by the **same** frame's `build_scope` drain of the U1 inbox.
  Wakers request a frame through the scheduler's existing `frame_scheduled` +
  `on_frame_scheduled` coalescing path. `TaskToken` cancels on drop, dropping the
  future. **One design correction:** `drive_async_tasks` does not set the scheduler
  phase — the phase machine is strictly forward-only (`MidFrameMicrotasks -> Idle` is
  illegal) and is driven by the desktop runner, which returns it to `Idle` *before*
  `AppBinding::draw_frame` runs. It debug-asserts the invariant that matters instead:
  never poll during `PersistentCallbacks` (build/layout/paint).
- **U3 — `AsyncSnapshot` / `ConnectionState`** (`flui-foundation`), with the D4 tables
  as unit tests over the fold functions alone. ✅ **Landed 2026-07-09.** Pure data in
  `flui-foundation` (next to `Listenable`/`ValueNotifier`), no futures and no executor.
  `T`/`E` carry **no bounds**: a generic fn with no `where` clause drives every
  constructor, fold, and accessor, so a `Clone`/`Copy` bound can never creep in.
  Flutter's `assert(data == null || error == null)` is upheld by construction (private
  fields; `with_data` clears the error, `with_error` clears the data). 27 tests
  transcribe the D4 tables, including the `SynchronousFuture` `Done`-guard in
  `after_subscribe` and the `in_state` payload preservation that makes
  `'ignores initialData when reconfiguring'` true. Divergences as designed: no stack
  trace, no `require_data`, snapshot read by reference.
- **U4 — `FutureBuilder`**, private module, full transition suite. ✅ **Landed 2026-07-09.**
  `pub(crate)` in `crates/flui-view/src/element/future_builder.rs`; nothing exported from
  `flui-view`'s root or `flui-widgets`. Composes U1 (`RebuildHandle`, captured in
  `init_state`), U2 (`AsyncDriver`), U3 (`AsyncSnapshot`). Builder receives
  `&AsyncSnapshot<T, E>`; `T`/`E` need no `Clone` (proved by a non-`Clone`, non-`Copy`
  payload flowing through the constructor, factory, completion path, and builder).
  16 tests, red-checked.

  **Three corrections found while implementing:**

  1. **The driver was unreachable from a widget.** `HeadlessBinding` drives a
     binding-local `Scheduler`; production drives `Scheduler::instance()`. A widget
     reaching for the singleton would spawn into a driver that never runs headlessly —
     silent divergence, precisely what ADR-0017 U4 warned about. Fixed by plumbing the
     driver: `BuildOwner::set_async_driver` (installed by both bindings) →
     `ElementOwner` → `BuildCtx` → **`BuildContext::async_driver() -> Option<AsyncDriver>`**.
     `None` when no binding installed one, reported honestly rather than by spawning
     into a dead driver.

  2. **`spawn_local` cannot honour the `SynchronousFuture` window.** `init_state` runs
     inside `build_scope`, and the frame's driver step already ran *before* it, so a
     ready future would first be polled next frame and the first build would show
     `Waiting`. Added **`AsyncDriver::spawn_local_eager`**, which polls once inline at
     subscribe time (returning `None` when that poll completes the future) — exactly as
     Dart's synchronous `.then` runs inline inside `initState`. A wake landing during
     that inline poll finds no task in the map, so the stale-waker guard suppresses its
     frame request; `spawn_local_eager` re-requests once the task is live.

  3. **`make` is `Fn`, not the ADR's `FnOnce`.** A view is cloned on every rebuild, so
     the factory cannot be `FnOnce`. It is called once per subscription. Likewise
     `initial_data` is an `Fn() -> T` factory rather than a `T`, so `T` needs no `Clone`
     to sit inside a cloneable view.

  **On the generation counter:** it is genuinely unreachable through the widget, because
  dropping the `TaskToken` cancels the task before its writer can run. Rather than ship
  untested defensive code, the writer is factored into `apply_completion(slot, generation,
  result) -> bool`, which is unit-tested for the current, stale, and inline-window cases.
  It remains as defence in depth for a leaked token or a future that resolves off-thread —
  the same reason Flutter carries `_activeCallbackIdentity`, which for Dart is the *only*
  defence, since Dart cannot cancel a future at all.
- **U5 — `StreamBuilder`**, private module, full fold suite. ✅ **Landed 2026-07-09.**
  `pub(crate)` in `crates/flui-view/src/element/stream_builder.rs`; nothing exported.
  Same seams and same keyed identity as U4. `futures-core` added as a **trait-only**
  workspace dependency (no executor, no combinators, no proc macros) — it was already in
  the graph via wgpu's `flume`, so `Cargo.lock` did not change. Because `futures-core`
  ships no `StreamExt::next()`, the task polls the stream by hand through
  `std::future::poll_fn`, which is exactly why the trait-only dep suffices. 17 tests,
  red-checked.

  **Correction found while implementing — `StreamBuilder` must NOT poll eagerly.**
  U4 subscribes with `spawn_local_eager` to reproduce Dart's synchronous `.then`
  (`SynchronousFuture`). A stream must not: `_StreamBuilderBaseState._subscribe` calls
  `listen(...)` and then `afterConnected` **unconditionally** (no `Done` guard, unlike
  `FutureBuilder`'s `after_subscribe`), and Dart's `Stream.listen` never delivers an event
  synchronously. An eager inline poll could yield an item *before* `after_connected` ran,
  and `after_connected` would then drag `Active` back to `Waiting`. So `StreamBuilder` uses
  plain `spawn_local` — first polled on the next frame's driver step, exactly as a Dart
  stream's first event arrives in a later microtask. Pinned by
  `stream_builder_shows_waiting_before_the_first_event`, which queues an event *before*
  mounting and asserts `Waiting` is still observed.

  **Shared slot.** `Slot`/`SharedSlot`/`apply_fold` moved to `element/async_slot.rs` so
  both builders share one channel and one generation guard rather than two divergent
  copies. `FutureBuilder`'s 16 tests still pass unchanged.

  **`after_disconnected` is only observable on the transition to an absent stream.** On a
  key change with a resubscribe it is a no-op in FLUI (`Active` → `None` → `Waiting`
  collapses to `Active` → `Waiting`, since `in_state` preserves the payload and only the
  final state reaches `build`). It is kept because Flutter does it, and because it *is*
  load-bearing when the new key is `None` — verified by red-checking
  `stream_builder_transition_to_absent_stream_cancels_and_preserves_payload`.

  **`FR-036` registry entry.** `dyn Stream` joins `dyn Future`/`dyn Iterator` as a
  language-runtime exempt in port-check trigger 9, documented in `docs/PORT.md`. Verified
  the trigger still rejects an unsanctioned `Box<dyn Foo>`.
- **U6 — parity re-check against `.flutter/`, then** public export from `flui-widgets`
  + prelude, and only then flip tracker B1.1. ✅ **Landed 2026-07-09.** Both builders are
  public; the `#![allow(dead_code)]` attributes U4/U5 needed are gone. 13 public tests
  (`crates/flui-widgets/tests/{future,stream}_builder.rs`) drive the real
  `flui_widgets::prelude` surface through a real `HeadlessBinding` frame.

  **One correction found by the public tests.** `flui-widgets`' `lay_out` harness mounted
  the tree — running `init_state`, where a builder subscribes — *before*
  `HeadlessBinding::with_tree` installed the async driver on the `BuildOwner`. Every
  subscription would have hit the "no async driver" warn path and silently never polled.
  Fixed by splitting `HeadlessBinding::bind_tree` out of `with_tree`, so the harness can
  take the driver from `binding.scheduler().async_driver()` *before* mounting. Red-checked:
  removing the install fails `future_builder_pending_then_success`. `with_tree` still works
  and now delegates to `bind_tree`.

  **`Stream` is re-exported** as `flui_widgets::Stream`, so a consumer can implement or
  name a stream without depending on `futures-core` directly. Still trait-only; no
  `futures-util`, no runtime.

  **`FutureBuilderState` / `StreamBuilderState` are `pub` but opaque** — no public fields,
  no public methods. Rust forbids a crate-private type as the `State` associated type of a
  public `StatefulView` impl; that is the only reason they are nameable.

---

## Parity findings (U6)

Re-read `.flutter/packages/flutter/lib/src/widgets/async.dart` and
`.flutter/packages/flutter/test/widgets/async_test.dart` (Flutter master
`3.33.0-0.0.pre-6280-g88e87cd963f`) against the landed implementations.

**`FutureBuilder` — all eight oracles match.** `initialData` seeds `with_data(None, d)`;
absent future never subscribes and holds that snapshot (`'gracefully handles transition to
null future'`); `SynchronousFuture` ⇒ `Done` on the first build with no `Waiting`
(`spawn_local_eager`); success/error ⇒ `Done` + data / `Done` + error, mutually exclusive;
same identity ⇒ `didUpdateWidget` early-return, no resubscribe; identity change ⇒
`inState(none)` → `after_subscribe` → `Waiting` with the **old** payload preserved and
`initialData` **not** re-applied (`'ignores initialData when reconfiguring'`); transition to
a null future cancels and leaves `None` + old payload; a stale completion cannot mutate the
snapshot (the `TaskToken` cancels first; the generation guard backs it up).

**`StreamBuilder` — all eight oracles match.** `initialData` as above; absent stream never
subscribes; `listen` → `afterConnected` is **unconditional**, so `Waiting` always precedes
the first event (hence plain `spawn_local`, never `spawn_local_eager`); the fold sequence
`Active(d)` → `Active(err)` → `Active(d)` → `Done` holds, with `after_data` clearing a stale
error and `after_error` clearing stale data, and `after_done` preserving the last payload
*or* error; same identity ⇒ no resubscribe; identity change ⇒ `afterDisconnected` →
`afterConnected` preserving the old payload, seed not re-applied; transition to a null stream
cancels the task and leaves `None` + payload; stale events are discarded.

**Divergences carried forward** (all previously recorded, none discovered here):

| Flutter | FLUI | Why |
|---|---|---|
| `future`/`stream` identity via `==` | explicit `key: Option<K>` + factory | A Rust `Future`/`Stream` is move-only, not `Clone`, not `Eq`, and cannot live in a view cloned each rebuild. See the sign-off below. |
| `error: Object?` + `StackTrace` | generic `E`, no stack trace | Rust has no ambient stack traces; errors live in `Result<T, E>`. |
| `requireData` throws | absent | Dart lacks `Option`. `snapshot.data()` → `Option<&T>`. |
| snapshot by value | snapshot by **reference** | Avoids `T: Clone`; FOUNDATIONS forbids the Druid bound-creep trap. |
| `FutureBuilder.debugRethrowError` | absent | Debug hook re-throwing into a Dart zone; no analogue. |
| `hasData` false for `Future<void>` | `has_data()` true for `Future<()>` | `Some(())` is data; Dart's `null` is not. Documented on `AsyncSnapshot::has_data`. |
| `StreamBuilderBase<T, S>` | absent | Deferred until a second real consumer appears. |
| widget rebuild + constraints change ⇒ builder runs once | (`LayoutBuilder` only, ADR-0017) | n/a here. |

---

## Sign-off: the keyed-identity public API (U6 Gate 2)

**Signed off 2026-07-09 by the repository owner (`vanyastaff`)**, in the U6 task
authorization, which specifies the shape verbatim (`FutureBuilder::keyed(key, make,
builder)` / `StreamBuilder::keyed(key, make, builder)`, `with_initial_data` `Clone`-free,
snapshot by reference, no `Clone`/`Copy` on `T`/`E`) and directs export once Gate 1 passes.
This repository has no separate api-design-lead role; the owner is the deciding authority,
and this note records that rather than implying a review that did not occur.

The divergence is **forced, not chosen**: `oldWidget.future == widget.future` cannot be
written in Rust. It is also the better API — Flutter's most notorious `FutureBuilder` bug is
calling an `async` function inside `build`, which mints a new `Future` every rebuild and
re-enters `waiting` forever. The keyed form makes that unrepresentable: the factory runs once
per subscription, and a subscription is recreated exactly when the key changes.

Migration cost if reversed: total. That is why it is recorded here before export.

---

## Correction (2026-07-10, ADR-0021 U1.5): the driver step moved into the `Scheduler`

This ADR stated the mid-frame async-driver contract as *"exactly one driver step per
frame, **owned by the binding**"* (`Scheduler::drive_async_tasks`'s doc comment, and
`handle_begin_frame_does_not_poll_async_tasks`). Both bindings duly called
`Scheduler::drive_async_tasks()` themselves — `HeadlessBinding::pump_frame` and
`AppBinding::draw_frame`.

**The ownership was never the contract.** The invariant that matters is:

> exactly one mid-frame poll per frame, on the **right `Scheduler` instance**, after
> the transient callbacks and before the persistent ones.

Locating the *call* in the bindings was a way of achieving it, and it turned out to
block a correctness fix. `AppBinding::draw_frame` **is** the pipeline, and the
pipeline must run in the scheduler's `PersistentCallbacks` phase for post-frame
callbacks to observe its committed layout (ADR-0021 §7b). But
`drive_async_tasks` debug-asserts it is *never* called during `PersistentCallbacks`
— the very phase the pipeline needs. A binding that both polls the driver and runs
the pipeline cannot satisfy both.

**Resolution.** `Scheduler::handle_begin_frame` now performs the poll itself, in the
`MidFrameMicrotasks` slot, and the bindings no longer call it. The invariant is
enforced *structurally* rather than by trusting two call sites:

* one poll per frame — there is one call, in the scheduler;
* on the right instance — `HeadlessBinding` drives its binding-local `Scheduler`,
  the runners drive the `Scheduler::instance()` singleton, and each polls only its
  own `AsyncDriver`;
* still between transient and persistent — `handle_begin_frame` ends there.

ADR-0017 U4's rule ("headless and production must share one frame-step
implementation") is *strengthened*: both now go through `Scheduler::drive_frame`.

**Tests changed.** `handle_begin_frame_does_not_poll_async_tasks` and
`draw_frame_invokes_the_async_driver_step` pinned the old call site and are replaced
by tests that pin the invariant instead:
`handle_begin_frame_polls_async_tasks_once_in_the_mid_frame_phase`,
`each_scheduler_instance_polls_only_its_own_async_driver`,
`the_production_frame_polls_the_singletons_async_driver_once_before_the_pipeline`,
`draw_frame_does_not_poll_the_async_driver_itself`, and
`pump_frame_still_polls_the_async_driver_exactly_once_per_frame`.

**Consequence, stated plainly.** Calling `AppBinding::draw_frame` *outside*
`Scheduler::drive_frame` now polls no async tasks. Every frame driver goes through
`drive_frame`; a caller that hand-rolls a frame does not get the driver step.
