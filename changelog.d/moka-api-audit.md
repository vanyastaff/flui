### Changed

- Asset cache capacity now explicitly selects disabled retention or a nonzero entry count instead of guessing entries from bytes; the default remains 10,240 entries per asset type.
- Asset cache expiration is configurable through checked intervals, including no expiration and immediate expiration.
- Cache presence is synchronous and does not refresh idle expiration, popularity or request counters; estimated lengths are now `u64`.
- Generic cache initialization keeps owned errors and independent get/load/insert publication, permitting direct and awaited spawned-task reentry. Image and custom registry cold loads now run independently: every success publishes, late completion may replace cached data, and existing handles stay valid. Closed built-in FontAsset retains pending-load coalescing and cancellation recovery; registered image decoder hooks remain supported, and exactly-once callback side effects are not promised.
- Cache statistics name explicit invalidation requests `invalidations`; utilization uses configured capacity and rates remain finite at saturated counters.
- Require Moka 0.12.16, which fixes concurrent removal/admission capacity accounting.

### Fixed

- Registry-wide cache clearing retires generic data after releasing the registry guard so data destructors can reenter safely.
- Same-key initializer reentry through a cache clone makes progress after suspension; successful nested calls publish independently, and the outer completion can replace their data. Awaited native spawned-task reentry also terminates.
