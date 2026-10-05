### Fixed

- Retire final notification-channel callbacks individually and protect separately owned values during failed `ValueNotifier` disposal or extraction, preserving healthy destruction and clone ownership.

### Changed

- Borrowed values inside `ValueNotifier<T>` must remain valid through notifier destruction. Declare the notifier inside its referent's scope, or keep the referent alive longer; owned and non-Clone values and extraction remain supported.
