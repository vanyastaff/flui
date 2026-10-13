### Added

- **`Notifier::retire_after_delivery` and `ChangeNotifier::retire_after_delivery`**
  (`flui-foundation`): hand a capture to the delivery in progress, which retires it when the round
  ends and retains it if the round fails.

### Fixed

- **A value listener removed during a status delivery waits for the round** (`flui-animation`): a
  curved, tween, reverse, proxy or switch animation that delivers a status keeps a value listener
  removed from it until that round ends, so a later status observer's failure retains its capture
  instead of finding it already destroyed.
