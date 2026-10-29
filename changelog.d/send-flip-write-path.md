### Added
- `BuildOwner::writer_source` and `BuildOwner::writer_source_for(slot)`: host code writes a signal outside a widget through an `EventCx` opened on the owner's graph, the way an event callback does.
- `HeadlessBinding::writer_source`, replacing `HeadlessBinding::reactive`.
- With `flui-view`'s `testing` feature: `BuildOwner::signal_graph`, for tests that mint signals outside an element or inspect the reader registry. It cannot write.
- `PostFrameHandle::schedule_or_return` and `PostFrameRejection`: if an owner-local obligation's queue has closed, the obligation comes back to the caller unrun. If the owner closed with a failure in custody, it stays retained instead.

### Removed
- `Reactive` is no longer a signal write target: `count.set(&graph, v)` becomes `source.write(|cx| count.set(cx, v))`.
- `BuildContext::reactive()` and the public `BuildOwner::reactive()`. A lifecycle hook acquires `ctx.writer_source()`; a test reaches the graph through the `testing` feature.
- `HeadlessBinding::reactive()`; use `HeadlessBinding::writer_source()`.

### Changed
- A cross-thread signal write (`UiCommand::SignalWrite`) runs inside an `EventCx` opened by the `WriterSource` of the presentation that minted its slot, instead of on the raw graph.

### Fixed
- A local-history entry's `on_remove` owed by a pop applied during `build` runs after the frame, so the signal write it makes through its captured `WriterSource` is no longer refused and lost.
- Each owed `on_remove` runs in its own post-frame entry, so one that panics no longer keeps the next from running. A navigator that has unmounted severs the `on_remove` callbacks it still owes instead of running them inside another element's `build`.
