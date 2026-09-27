# flui-cupertino Architecture

The official Cupertino catalog builds on `flui-sdk`. Widget configuration
and callbacks are owner-local; the package does not own a reactive graph.

## Mapping decisions

### Event callbacks borrow the originating dispatch's write context

`CupertinoButton` press/long-press and `CupertinoTabBar` selection handlers
take `&mut EventCx` and return an `EventOutcome` (ADR-0086). Gesture wrappers
forward their context without opening a new writer. `CupertinoTabScaffold`
updates its controller before forwarding the same context and selected
index to the caller. Tab builders remain read-only queries.

The button's press animation still starts before its callback. The integration
test `tap_callback_writes_a_signal_and_rebuilds_its_reader` drives the mounted
button through pointer dispatch and checks the signal and subsequent rebuild.
