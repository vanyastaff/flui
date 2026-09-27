### Added

- **`flui-view`**: typed signal writes
  ([ADR-0086](/docs/adr/ADR-0086-signal-writes-through-event-context.md)). An event callback
  receives `&mut EventCx<'_>` and writes through it (`move |cx| count.set(cx, 1)`). A widget
  opens one from a `WriterSource`, acquired in `init_state` with the new
  `LifecycleContext::writer_source`, which `build` cannot reach. Also added: `Writer`,
  `WriteTarget`, `EventOutcome` (a callback may return a write's `Result`; a refused write is
  logged on `flui::signals`) and `callback`, which fixes the signature of a `let`-bound event
  closure. `EventCx`, `Signal`, `WriterSource` and `callback` are in the prelude.
- **`flui-widgets`**: `RawButton`, a theme-free button whose `on_press` receives the
  `&mut EventCx<'_>`. It publishes button semantics, is reachable by a platform click, and is
  disabled without a callback.
- **`flui-foundation`**: `Signal::default()`, an unbound handle a `ViewState` can hold until
  `init_state` creates the signal; every read or write through it is `SignalError::Unbound`.

### Changed

- **`flui-view`**: `SignalWriteExt::set`, `update` and `set_if_changed` take any `WriteTarget`;
  existing `&Reactive` calls compile unchanged.
- **Examples**: `counter` and `todo` use the widgets catalog, `Signal` state and `RawButton`, and
  no longer need `--features material`. The `flui create` counter template generates the same
  code as `counter`.
