### Changed

- `Animation::subscribe_status` is `listen_status` and `Animation::observe_status` is
  `listen_status_with_recovery`, beside `Listenable`'s `listen` and `listen_with_recovery`. Both
  return `flui_foundation::Subscription` (ADR-0379).
- `Subscription::detach` on a status subscription removes the callback at once, as it does on
  every other subscription. A callback meant to live as long as its animation calls
  `Subscription::forget`.

### Added

- `Subscription::forget`: the listener stays registered until its source is disposed or retired.
- `Subscription::detach_with_recovery`: a detach inside a cleanup round already under way, which
  keeps that round's first failure.

### Removed

- `flui_animation::StatusSubscription`: status subscriptions are the foundation `Subscription`.
  A custom animation builds one with `Subscription::for_source`, and `cancel_with_recovery` is
  `detach_with_recovery`.
