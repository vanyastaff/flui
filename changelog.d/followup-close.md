### Fixed

- **Presentation close** (`flui-runtime`): an ordinary close keeps the withdrawn custom
  `GlobalKey` owners until focus, text input, gestures and mouse tracking are closed, so a key's
  destructor can no longer drive a saved `FocusNode` or `TextInputHandle` of the closing
  presentation ([ADR-0123](/docs/adr/ADR-0123-exceptional-presentation-close.md)).
- **Text input** (`flui-interaction`): attaching over, or detaching, a client releases the local
  platform-capability clone before the client retires, so a store that closes its owner and then
  panics no longer leaves that clone to be destroyed by the unwind.
- **Lifecycle subscriptions** (`flui-view`): a preserving close retains callbacks rejected through
  stale handles only while that close is in progress; afterwards they retire normally again.
- **Interaction dispatch** (`flui-interaction`): scroll, pan-zoom, path-clip and shader-mask
  invocations retain their snapshot and target cell when the callback closes its presentation and
  panics, instead of destroying them during the unwind
  ([ADR-0127](/docs/adr/ADR-0127-exceptional-path-retention.md)).
- **Interaction dispatch** (`flui-interaction`): a presentation-scoped handle can replace,
  unregister or detach only the targets its own owner registered.
- **Signals** (`flui-view`): an element releasing its signals refuses new signals owned by it, so
  a value whose destructor recreates itself no longer keeps the release looping forever.
