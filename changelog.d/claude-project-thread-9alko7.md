### Changed

- **`flui-view`**: `StatefulView::create_state` takes the `&dyn LifecycleContext` that
  `init_state` receives, and runs at the element's first build, right before `init_state`,
  from the element's current view. A state creates its signals and capability handles there:
  `CounterState { count: ctx.signal(0) }`. `StatefulBehavior::new` and `AnimatedBehavior::new`
  take no view; `StatefulElement::state` and `state_mut` return `Option`, `None` until the first
  build.

### Removed

- **`flui-foundation`**: `Signal::default()`, the unbound placeholder a state held until
  `init_state` replaced it, with `SignalError::Unbound` and `SignalSlot::is_unbound`.
  `SignalSlot::UNBOUND_GRAPH` is `SignalSlot::NO_GRAPH`.
- **`flui-view`**: `StatefulElement::set_state` and `set_state_scheduled`, which no caller used.
