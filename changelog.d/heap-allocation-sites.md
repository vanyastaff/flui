### Added

- `flui_testing::heap::sites`: with flui-testing's `dhat` feature, `CountingAllocator` forwards to DHAT's allocator, and `SiteProfiler` writes a DHAT profile of a stretch of work and sums its allocations per line of the calling code (`AllocationSites`). The `app_scenarios` bench prints those sites per scenario when built with `--features flui-testing/dhat`.

### Fixed

- `CountingAllocator` counts only the program's requests when its inner allocator allocates for itself.
