### Fixed

- Disposed controller value-listener identity exhaustion releases the state
  borrow before reporting failure and preserves opaque callback custody.
