### Fixed

- Asset and network image providers reuse completed decoded images during async resolution, avoiding a second load after a successful decode. Cold failures remain retryable.

### Changed

- Decoded-image cache insertion returns evicted entries before freeing their pixel buffers outside the cache mutex. Cache hits retain LRU recency and eviction preserves displayed image handles.
- Require lru 0.18.5 or newer, which narrows its hashbrown feature requirements.
