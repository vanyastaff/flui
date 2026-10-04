### Changed
- Concurrent registry loads of the same typed asset key share work and allocation. Failed loads remain retryable, and a waiting request can progress after initializer cancellation.
### Removed
- Removed `AssetRegistry::stats`, which always returned an empty vector; typed `AssetCache::stats` remains available.
