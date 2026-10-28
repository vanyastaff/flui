### Added
- `BuildOwner::writer_source` and `BuildOwner::writer_source_for(slot)`: host code writes a signal outside a widget through an `EventCx` opened on the owner's graph, the way an event callback does.
- `HeadlessBinding::writer_source`, replacing `HeadlessBinding::reactive`.
- With `flui-view`'s `testing` feature: `BuildOwner::signal_graph`, `BuildContextExt::signal_graph` and `Reactive::writer_source`, for tests that mint signals outside an element or inspect the reader registry.

### Removed
- `Reactive` is no longer a signal write target: `count.set(&graph, v)` becomes `source.write(|cx| count.set(cx, v))`.
- `BuildContext::reactive()` and the public `BuildOwner::reactive()`. A lifecycle hook acquires `ctx.writer_source()`; a test reaches the graph through the `testing` feature.
- `HeadlessBinding::reactive()`; use `HeadlessBinding::writer_source()`.

### Changed
- A cross-thread signal write (`UiCommand::SignalWrite`) runs inside an `EventCx` opened by the `WriterSource` of the presentation that minted its slot, instead of on the raw graph.

### Fixed
- A local-history entry's `on_remove` owed by a pop applied during `build` runs after the frame, so the signal write it makes through its captured `WriterSource` is no longer refused and lost.
