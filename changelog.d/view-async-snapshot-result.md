### Changed

- Moved `AsyncSnapshot<T, E>` to `flui-view` and replaced the separate connection/data/error state with `None`, `Waiting`, `Active`, and `Done` variants that retain the latest `Result` without requiring `T: Clone` or `E: Clone`. Key changes preserve that result, and stream completion distinguishes an empty stream from a final result.

### Removed

- Removed `ConnectionState` and the compatibility constructors and accessors for the previous connection/data/error snapshot representation.
