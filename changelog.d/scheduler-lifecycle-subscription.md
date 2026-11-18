### Added

- `flui_foundation::Notifier::subscribe`: the argument-taking form of `listen`, whose
  `Subscription` detaches the listener when it drops.
- `UpdateScheduler::listen_lifecycle_state`: hear each lifecycle state the scheduler commits for as
  long as the returned `Subscription` is held.

### Removed

- `UpdateScheduler::add_lifecycle_state_listener`, `UpdateScheduler::remove_lifecycle_state_listener`
  and `flui_scheduler::LifecycleStateCallback`: a lifecycle listener is held by its
  `Subscription` instead of an id removed by hand.
