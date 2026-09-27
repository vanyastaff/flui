### Fixed

- Bound owner-thread realm dispatch to finite batches with one coalesced continuation, preventing self-enqueueing callbacks from starving native UI work while preserving terminal close ordering.
