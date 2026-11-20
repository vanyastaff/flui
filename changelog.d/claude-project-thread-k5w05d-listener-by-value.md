### Changed

- `flui-foundation`: `Notifier::take_callback`, `take_all_callbacks` and
  `dispose_and_take_callbacks`, `ChangeNotifier::take_listener`, `take_listeners` and
  `dispose_and_take_listeners`, and `NotificationCallback::user` and `relay` hand a
  `NotificationCallback<Arg>` over by value instead of an `Rc<NotificationCallback<Arg>>`. A
  channel holds each listener's closure as it was given, so a subscription no longer allocates
  an envelope; a delivery's snapshot shares the closure, and a listener removed during its own
  call still keeps its captures until the round's tail. The control cost this lowers is in
  [`raw_button_over_text.toml`](/crates/flui-widgets/perf/baseline/raw_button_over_text.toml).
