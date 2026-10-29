### Changed
- `CloseGuard` now records a close a hold refuses as a `PendingClose`, one per `CloseReason`, and makes it due once when the last hold is released or the close is discarded; `CloseGuard::changed` resolves on the owner's next settle.
- A `BuildOwner` dropped with a tree observer installed calls the observer's `detached()` once.
