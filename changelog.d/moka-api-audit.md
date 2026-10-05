### Changed

- Asset cache capacity now explicitly selects disabled retention or a nonzero entry count instead of guessing entries from bytes; the default remains 10,240 entries per asset type.
- Asset cache expiration is configurable through checked intervals, including no expiration and immediate expiration.
- Cache presence is synchronous and does not refresh idle expiration, popularity or request counters; estimated lengths are now `u64`.
- Public cache initializers coalesce concurrent loads and return shared `Arc<Error>` failures without requiring cloneable errors. Registry loading retains its owned error contract.
- Cache statistics name explicit invalidation requests `invalidations`; utilization uses configured capacity and rates remain finite at saturated counters.
- Require Moka 0.12.16, which fixes concurrent removal/admission capacity accounting.

### Fixed

- Registry-wide cache clearing retires generic data after releasing the registry guard so data destructors can reenter safely.
- Same-key initializer reentry through a cache clone makes progress after suspension; nested initialization returns uncached data while the outer initializer retains publication ownership.
