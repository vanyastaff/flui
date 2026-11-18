### Changed

- **`Animation` has one required status method** (`flui-animation`): implementors write
  `listen_status_with_recovery(StatusObserver)`, which receives the notifying delivery's `RecoveryScope`;
  `listen_status(StatusCallback)` is now a provided adapter over it, so callers are unchanged.
  `StatusObserver` is documented public API. The hidden `subscribe_status_observer`, whose default
  dropped the enclosing recovery context for custom animations, is gone.
- **`Notifier` and `ChangeNotifier` relay methods are public API** (`flui-foundation`):
  `ChangeNotifier::notify_listeners_with_recovery`, `take_listener`, `take_listeners` and
  `inherit_failure`, and `RecoveryCallback`,
  `NotificationCallback`, `Notifier::add_with_recovery`, `Notifier::notify_with_recovery`,
  `Notifier::take_callback` and `Notifier::inherit_failure` are documented, so a custom status
  channel can implement `listen_status_with_recovery` and its recovery-aware removal without hidden items.
