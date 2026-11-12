### Changed

- `flui_foundation::Arity`, `MonotonicClock`, `TreeObserver`, `Marker` and `TreeId` no longer
  require `Send + Sync`: each is used on the owner thread (ADR-0323 §4). A clock or tree observer
  may now hold `Rc` and `Cell` state.

### Removed

- `flui_foundation::WasmNotSendSync`. A bound that names a thread is written as `Send` where a
  value crosses one, and is absent everywhere else.
