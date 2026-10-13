### Changed

- **`Animation` has one required status method** (`flui-animation`): implementors write
  `observe_status(StatusObserver)`, which receives the notifying delivery's `RecoveryScope`;
  `subscribe_status(StatusCallback)` is now a provided adapter over it, so callers are unchanged.
  `StatusObserver` is documented public API. The hidden `subscribe_status_observer`, whose default
  dropped the enclosing recovery context for custom animations, is gone.
- **`Notifier` relay methods are public API** (`flui-foundation`): `RecoveryCallback`,
  `Notifier::add_with_recovery` and `Notifier::notify_with_recovery` are documented, so a custom
  status channel can implement `observe_status` without hidden items.
