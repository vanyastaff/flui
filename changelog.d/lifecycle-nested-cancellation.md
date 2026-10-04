### Fixed

- Lifecycle delivery retains cancelled callback captures after an earlier callback failure, including cancellation from a later callback, while completing queued notifications and preserving ordinary cancellation on the next operation.
