# ADR-0018: Async builder seam — a `'static` rebuild handle, a frame-driven driver, keyed identity

- **Status:** Accepted
- **Date:** 2026-07-09
- **Superseded in part by:** ADR-0078 (capability acquisition)

## Context

Flutter's `FutureBuilder`/`StreamBuilder` rely on three things FLUI lacked:

1. **A rebuild from any callback.** `State.setState` reaches `Element.markNeedsBuild` through
   a back-reference. FLUI's `BuildContext::mark_needs_build` is borrow-scoped and cannot be
   captured by a `'static + Send` completion callback; `dispose` receives no context at all.
2. **Something that polls the future.** A Dart `Future` completes on the UI isolate's event
   loop. The FLUI view/widget/app stack owns no async runtime.
3. **Future identity.** `didUpdateWidget` compares `oldWidget.future == widget.future`. A Rust
   `Future` is move-only, not `Clone`, not `Eq`, and cannot live in a view that is cloned on
   every rebuild.

Reference: `widgets/async.dart` and `test/widgets/async_test.dart`.

## Decision

### D1. The subscription lives in `ViewState` as a drop-cancelling token

`FutureBuilder` and `StreamBuilder` are ordinary `StatefulView`s. `init_state` captures a
rebuild handle, seeds the snapshot, spawns the task and moves to `Waiting`; a key change in
`did_update_view` drops the old token and resubscribes; `dispose` drops the token. Because
`TaskToken` cancels on drop, a completed future can never write into a disposed state, and
cancellation needs no context.

### D2. Completion writes a cell and schedules a rebuild; it never touches a tree

Completion writes the new `AsyncSnapshot` into a shared slot, then calls
`RebuildHandle::schedule()`: insert the `ElementId` into the build owner's external inbox (a
set, so a burst of stream events collapses to one entry) and request a frame. `build_scope`
drains the inbox at frame start and the element's `build` reads the slot. Nothing rebuilds
during layout or paint; a wake landing mid-frame waits for the next frame.

`RebuildHandle` (`Clone + Send + Sync + 'static`) is the capability; `schedule()` is legal
from any thread and inert once its element is gone. It is acquired only in lifecycle hooks and
fired later from callbacks; ADR-0078 records how that rule is enforced. `AnimatedView` rides
the same channel.

### D3. A frame-driven driver in `flui-scheduler`, not a runtime

`AsyncDriver` (`std::future`/`std::task` only; no `tokio`, no `futures` executor) is polled
once per frame by `Scheduler::handle_begin_frame`, in the `MidFrameMicrotasks` slot: after
transient callbacks, before the persistent (build/layout/paint) phase, so a completion is seen
by the same frame's `build_scope`. The poll lives in the scheduler, not in the bindings, so
headless and production frames share one step and neither can skip or double it. It
debug-asserts it never runs during `PersistentCallbacks`. Wakers request a frame, so a
completion signalled from a worker thread wakes the UI.

This is Flutter's model: a Dart future's callbacks run on the UI isolate's loop, and polling on
the frame thread reproduces that. CPU-bound work is the caller's to offload in both frameworks.

`spawn_local_eager` polls once inline at subscribe time, so an immediately-ready future shows
`Done` on the first build with no `Waiting` (Flutter's `SynchronousFuture` case).
`StreamBuilder` uses plain `spawn_local`: Dart's `listen` never delivers synchronously and
`afterConnected` is unconditional, so `Waiting` always precedes the first event.

### D4. Snapshot model

`ConnectionState { None, Waiting, Active, Done }` and `AsyncSnapshot<T, E>` live in
`flui-foundation` as pure data with no bounds on `T`/`E`. `with_data` clears the error and
`with_error` clears the data, upholding Flutter's `data == null || error == null` by
construction. The `FutureBuilder` transitions and `StreamBuilder` folds (`initial`,
`after_connected`, `after_data`, `after_error`, `after_done`, `after_disconnected`) follow
`async.dart` one to one, including `in_state` preserving the payload across a resubscribe and
`initial_data` not being re-applied on reconfiguration.

A stream of `Result<T, E>` continues after an error, like a Dart stream without
`cancelOnError`. Dropping the token cancels the producer itself, where Dart only ignores late
callbacks. A generation counter on the slot backs up the cancellation, as Flutter's
`_activeCallbackIdentity` does.

### D5. Identity is an explicit key

```rust
FutureBuilder::keyed(key: Option<K>, make: FutureFactory<T, E>, builder: SnapshotBuilder<T, E>)
StreamBuilder::keyed(key: Option<K>, make: StreamFactory<T, E>, builder: SnapshotBuilder<T, E>)
```

`K: Clone + PartialEq + Send + Sync + 'static`. Same key: no resubscribe (Flutter's early
return). Different key: unsubscribe, `in_state(None)`, resubscribe, `Waiting`. `None`: no
subscription. The factory is an `Fn` called once per subscription, and `initial_data` is an
`Fn() -> T` factory, so the view stays `Clone` without `T: Clone`.

The divergence is forced (`==` on futures cannot be written), and it is the better API:
Flutter's most common `FutureBuilder` bug — calling an `async` function inside `build`, which
mints a new future each rebuild and re-enters `Waiting` forever — cannot be expressed.

## Flutter divergences

| Flutter | FLUI | Why |
|---|---|---|
| identity via `future ==` | `key: Option<K>` + factory | Rust futures are move-only and not `Eq` (D5) |
| `error: Object?` + `StackTrace` | generic `E`, no stack trace | errors are in `Result<T, E>`; no ambient stack traces |
| `requireData` throws | absent; `data()` returns `Option<&T>` | Dart lacks `Option` |
| snapshot by value | builder gets `&AsyncSnapshot<T, E>` | avoids `T: Clone` |
| `hasData` false for `Future<void>` | `has_data()` true for `Future<()>` | `Some(())` is data |
| `debugRethrowError` | absent | no Dart zone |
| `StreamBuilderBase<T, S>` | absent | no second consumer |

`FutureBuilderState`/`StreamBuilderState` are `pub` but opaque, only because a public
`StatefulView` cannot name a private `State`. `flui_widgets::Stream` re-exports the
`futures-core` trait (trait-only dependency, no executor).

## Consequences

- No runtime enters the widget dependency graph; `flui-scheduler` owns a small executor.
- A frame driver that bypasses `Scheduler::drive_frame` gets no async poll.
- The keyed shape is public API; reversing it would be a full migration.

## Alternatives rejected

- **A `tokio` runtime in `flui-app` reachable from widgets.** Puts a runtime in every widget
  consumer's graph and forces a flavor choice, and the completion still has to hop to the frame
  thread.
- **Polling in a post-frame callback.** A future completing during a frame would rebuild two
  frames later.
- **Storing the future** as `Arc<Mutex<Option<BoxFuture>>>` (leaks the cell into user code) or
  `futures::future::Shared` (needs `T: Clone` and gives no identity across rebuilds).
