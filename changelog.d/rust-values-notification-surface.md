### Changed

- Allow non-Clone owned values in `ValueNotifier` reading, mutation, listening and extraction; cloning still requires a cloneable value and shares listeners.

### Removed

- Remove the unused `ListenerRegistry` and `ListenerSubscription` notification wrappers.
