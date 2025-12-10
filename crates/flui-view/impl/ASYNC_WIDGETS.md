# Flutter Async Widgets

This document analyzes async widget patterns from Flutter's `async.dart`.

## StreamBuilderBase<T, S>

**Source:** `async.dart:44`

Base class for widgets that build based on Stream interaction.

```dart
abstract class StreamBuilderBase<T, S> extends StatefulWidget {
  const StreamBuilderBase({super.key, required this.stream});

  final Stream<T>? stream;

  /// Initial summary (before any events)
  S initial();

  /// Called when connected to stream
  S afterConnected(S current) => current;

  /// Called on data event
  S afterData(S current, T data);

  /// Called on error event
  S afterError(S current, Object error, StackTrace stackTrace) => current;

  /// Called when stream completes
  S afterDone(S current) => current;

  /// Called when disconnected from stream
  S afterDisconnected(S current) => current;

  /// Build widget from current summary
  Widget build(BuildContext context, S currentSummary);
}
```

### State Implementation

```dart
class _StreamBuilderBaseState<T, S> extends State<StreamBuilderBase<T, S>> {
  StreamSubscription<T>? _subscription;
  late S _summary;

  @override
  void initState() {
    super.initState();
    _summary = widget.initial();
    _subscribe();
  }

  @override
  void didUpdateWidget(StreamBuilderBase<T, S> oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.stream != widget.stream) {
      if (_subscription != null) {
        _unsubscribe();
        _summary = widget.afterDisconnected(_summary);
      }
      _subscribe();
    }
  }

  @override
  void dispose() {
    _unsubscribe();
    super.dispose();
  }

  void _subscribe() {
    if (widget.stream != null) {
      _subscription = widget.stream!.listen(
        (T data) {
          setState(() { _summary = widget.afterData(_summary, data); });
        },
        onError: (Object error, StackTrace stackTrace) {
          setState(() { _summary = widget.afterError(_summary, error, stackTrace); });
        },
        onDone: () {
          setState(() { _summary = widget.afterDone(_summary); });
        },
      );
      _summary = widget.afterConnected(_summary);
    }
  }

  void _unsubscribe() {
    _subscription?.cancel();
    _subscription = null;
  }
}
```

## AsyncSnapshot<T>

Summary type for `StreamBuilder` and `FutureBuilder`.

```dart
enum ConnectionState {
  none,     // Not connected to async computation
  waiting,  // Connected but no data yet
  active,   // Connected and received data (stream only)
  done,     // Computation completed
}

@immutable
class AsyncSnapshot<T> {
  const AsyncSnapshot._(this.connectionState, this.data, this.error, this.stackTrace);

  // Factory constructors
  const AsyncSnapshot.nothing() : this._(ConnectionState.none, null, null, null);
  const AsyncSnapshot.waiting() : this._(ConnectionState.waiting, null, null, null);
  const AsyncSnapshot.withData(ConnectionState state, T data) : this._(state, data, null, null);
  const AsyncSnapshot.withError(ConnectionState state, Object error, [StackTrace? stackTrace])
      : this._(state, null, error, stackTrace);

  final ConnectionState connectionState;
  final T? data;
  final Object? error;
  final StackTrace? stackTrace;

  // Convenience getters
  T get requireData => data!;
  bool get hasData => data != null;
  bool get hasError => error != null;

  // Transform snapshot
  AsyncSnapshot<T> inState(ConnectionState state) =>
      AsyncSnapshot<T>._(state, data, error, stackTrace);
}
```

## StreamBuilder<T>

Concrete StreamBuilder using AsyncSnapshot.

```dart
class StreamBuilder<T> extends StreamBuilderBase<T, AsyncSnapshot<T>> {
  const StreamBuilder({
    super.key,
    this.initialData,
    required super.stream,
    required this.builder,
  });

  final AsyncWidgetBuilder<T> builder;
  final T? initialData;

  @override
  AsyncSnapshot<T> initial() => initialData == null
      ? AsyncSnapshot<T>.nothing()
      : AsyncSnapshot<T>.withData(ConnectionState.none, initialData as T);

  @override
  AsyncSnapshot<T> afterConnected(AsyncSnapshot<T> current) =>
      current.inState(ConnectionState.waiting);

  @override
  AsyncSnapshot<T> afterData(AsyncSnapshot<T> current, T data) =>
      AsyncSnapshot<T>.withData(ConnectionState.active, data);

  @override
  AsyncSnapshot<T> afterError(AsyncSnapshot<T> current, Object error, StackTrace stackTrace) =>
      AsyncSnapshot<T>.withError(ConnectionState.active, error, stackTrace);

  @override
  AsyncSnapshot<T> afterDone(AsyncSnapshot<T> current) =>
      current.inState(ConnectionState.done);

  @override
  AsyncSnapshot<T> afterDisconnected(AsyncSnapshot<T> current) =>
      current.inState(ConnectionState.none);

  @override
  Widget build(BuildContext context, AsyncSnapshot<T> currentSummary) =>
      builder(context, currentSummary);
}
```

## FutureBuilder<T>

Builder for Future-based async operations.

```dart
class FutureBuilder<T> extends StatefulWidget {
  const FutureBuilder({
    super.key,
    required this.future,
    this.initialData,
    required this.builder,
  });

  final Future<T>? future;
  final T? initialData;
  final AsyncWidgetBuilder<T> builder;
}

class _FutureBuilderState<T> extends State<FutureBuilder<T>> {
  Object? _activeCallbackIdentity;
  late AsyncSnapshot<T> _snapshot;

  @override
  void initState() {
    super.initState();
    _snapshot = widget.initialData == null
        ? AsyncSnapshot<T>.nothing()
        : AsyncSnapshot<T>.withData(ConnectionState.none, widget.initialData as T);
    _subscribe();
  }

  @override
  void didUpdateWidget(FutureBuilder<T> oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.future != widget.future) {
      if (_activeCallbackIdentity != null) {
        _unsubscribe();
        _snapshot = _snapshot.inState(ConnectionState.none);
      }
      _subscribe();
    }
  }

  void _subscribe() {
    if (widget.future != null) {
      final callbackIdentity = Object();
      _activeCallbackIdentity = callbackIdentity;
      
      widget.future!.then<void>((T data) {
        if (_activeCallbackIdentity == callbackIdentity) {
          setState(() {
            _snapshot = AsyncSnapshot<T>.withData(ConnectionState.done, data);
          });
        }
      }, onError: (Object error, StackTrace stackTrace) {
        if (_activeCallbackIdentity == callbackIdentity) {
          setState(() {
            _snapshot = AsyncSnapshot<T>.withError(ConnectionState.done, error, stackTrace);
          });
        }
      });
      
      _snapshot = _snapshot.inState(ConnectionState.waiting);
    }
  }

  void _unsubscribe() {
    _activeCallbackIdentity = null;
  }
}
```

## FLUI Equivalents

### With Signals (Native Approach)

FLUI has built-in reactive primitives that replace these patterns:

```rust
// StreamBuilder equivalent - use Signal with stream subscription
pub struct StreamSignal<T> {
    value: Signal<Option<T>>,
    error: Signal<Option<Error>>,
    state: Signal<ConnectionState>,
}

impl<T: Clone + Send + Sync + 'static> StreamSignal<T> {
    pub fn from_stream(stream: impl Stream<Item = T> + Send + 'static) -> Self {
        let value = Signal::new(None);
        let error = Signal::new(None);
        let state = Signal::new(ConnectionState::Waiting);
        
        // Subscribe to stream in background
        spawn(async move {
            pin_mut!(stream);
            while let Some(item) = stream.next().await {
                value.set(Some(item));
                state.set(ConnectionState::Active);
            }
            state.set(ConnectionState::Done);
        });
        
        Self { value, error, state }
    }
}

// Usage in view
impl StatelessView for MyStreamWidget {
    fn build(&self, ctx: &impl BuildContext) -> impl IntoElement {
        let stream_signal = ctx.watch::<StreamSignal<MyData>>();
        
        match stream_signal.state.get() {
            ConnectionState::Waiting => Loading::new(),
            ConnectionState::Active | ConnectionState::Done => {
                if let Some(data) = stream_signal.value.get() {
                    DataView::new(data)
                } else if let Some(err) = stream_signal.error.get() {
                    ErrorView::new(err)
                } else {
                    Empty::new()
                }
            }
            ConnectionState::None => Empty::new(),
        }
    }
}
```

### Resource Pattern (FutureBuilder equivalent)

```rust
/// Async resource with loading/error/data states
pub enum Resource<T> {
    Loading,
    Ready(T),
    Error(Box<dyn std::error::Error + Send + Sync>),
}

pub struct AsyncResource<T> {
    state: Signal<Resource<T>>,
}

impl<T: Clone + Send + Sync + 'static> AsyncResource<T> {
    pub fn from_future(future: impl Future<Output = Result<T, Error>> + Send + 'static) -> Self {
        let state = Signal::new(Resource::Loading);
        
        spawn(async move {
            match future.await {
                Ok(data) => state.set(Resource::Ready(data)),
                Err(e) => state.set(Resource::Error(Box::new(e))),
            }
        });
        
        Self { state }
    }
    
    pub fn get(&self) -> Resource<T> {
        self.state.get()
    }
}

// Usage
impl StatelessView for UserProfile {
    fn build(&self, ctx: &impl BuildContext) -> impl IntoElement {
        let user = ctx.watch::<AsyncResource<User>>();
        
        match user.get() {
            Resource::Loading => Spinner::new(),
            Resource::Ready(user) => ProfileCard::new(&user),
            Resource::Error(e) => ErrorMessage::new(&e.to_string()),
        }
    }
}
```

### Comparison: Flutter vs FLUI

| Flutter | FLUI | Notes |
|---------|------|-------|
| `StreamBuilder<T>` | `StreamSignal<T>` | Signal wraps stream |
| `FutureBuilder<T>` | `AsyncResource<T>` | Resource pattern |
| `AsyncSnapshot<T>` | `Resource<T>` enum | Simpler enum |
| `ConnectionState` | Part of Resource/Signal | Integrated |
| `builder: (ctx, snapshot)` | Pattern match in build | More explicit |
| Manual subscription | Auto via signals | Less boilerplate |

### Advantages of Signal-Based Approach

1. **No manual subscription** - Signals handle lifecycle
2. **Automatic rebuilds** - No explicit `setState`
3. **Composable** - Combine multiple async sources with `computed`
4. **Type-safe** - Rust enums for state
5. **Cancellation** - Drop signal = cancel subscription
