### Added

- **`Subscription`** (`flui-foundation`): an owned registration on a listener
  channel — cancel-on-drop, `detach` for an early exit, bound to the channel
  it subscribed on, so its removal always names its own channel's listener
  and never another channel's same-valued id. A channel that is disposed or
  already gone detaches quietly; a detach that lands inside the channel's own
  notify round retires through that round's containment.
- **`listen`** (`flui-foundation`): `ChangeNotifier::listen`, `ValueNotifier::listen` and
  `Notifier::listen` subscribe with the owned handle (`Subscription`). The
  `Listenable` trait's id API (`add_listener`/`add_observer` with a
  `ListenerId`, `remove_listener`, `remove_all_listeners`) is untouched
  this change; it leaves together with the manual pairings in the
  subscriptions theme's next step, which hoists `listen` onto the trait and
  folds the remaining per-source guard types (the animation status
  subscription's, ADR-0180) into this one.