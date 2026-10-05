### Fixed

- A panicking listener capture or `ValueNotifier` value no longer aborts the process when its notifier is cleared, disposed, extracted or dropped: the first panic propagates and the remaining callbacks are retired in registration order or retained.

### Changed

- Listener captures and `ValueNotifier` values that would be dropped while a notifier is already unwinding, or after an earlier capture or value destructor failed in the same operation, are now retained and never dropped ([ADR-0127](/docs/adr/ADR-0127-exceptional-path-retention.md)). Their destructors do not run, so resources they own (a channel sender, a file handle) stay open until process exit.
- `ValueNotifier<T>` now implements `Drop`, so borrowed data inside `T` must outlive the notifier. Declare the notifier after its referent in the same scope, or keep the referent alive longer; owned, non-`Clone` and non-`'static` values, and `into_value`, remain supported.
