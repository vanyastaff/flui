### Removed

- `flui_foundation::Notifier::add`, `Notifier::remove` and `Notifier::remove_even_if_disposed`, and
  `ChangeNotifier::add_listener` and `ChangeNotifier::remove_listener`: a listener is held by the
  `Subscription` that `listen` or `subscribe` returns. A source that must hand a withdrawn
  listener's custody to a `RecoveryScope` (a custom animation's `StatusSubscription`) registers
  with `add_with_recovery` and withdraws with `take_callback` or `take_listener`.
