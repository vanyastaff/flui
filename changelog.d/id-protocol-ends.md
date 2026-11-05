### Removed

- The id-returning methods are off the `Listenable` trait: `add_listener`,
  `add_observer`, `remove_listener` and `remove_all_listeners` lose their last
  trait implementations and their transition adapters, so the trait carries
  exactly the owned pair — `listen` and `listen_with_recovery`, each returning
  a `Subscription` (`/../docs/adr/ADR-0109-notification-channel-surface.md`
  decision 5). Out-of-tree implementors keep one registration shape: subscribe
  through the owned pair, hold the subscription, and let its drop detach.

### Changed

- The channel level keeps the id machinery the trait used to wrap:
  `ChangeNotifier::add_listener`/`add_with_recovery`/`remove_listener`/
  `remove_all_listeners` are documented channel methods for owners that
  retire through their own custody (ADR-0219), not trait protocol.
- `ProxyAnimation` loses `remove_listener_with_recovery` with its last users;
  a withdrawal that must join an enclosing cleanup runs the subscription's
  drop inside `recovery.run` or through a custom source's
  `Subscription::for_removal`.

### Fixed

- `AnimationController::listen` on a disposed controller retires the callback
  and answers an inert subscription, matching `listen_with_recovery`'s refusal
  contract (ADR-0180), instead of registering on the disposed channel.
