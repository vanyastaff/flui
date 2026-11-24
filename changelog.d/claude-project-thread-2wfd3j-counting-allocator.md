### Added

- `flui_foundation::testing::heap` (feature `testing`): `CountingAllocator`, the one counting
  global allocator, with `HeapCounts`, `HeapCounts::during`, `CountingAllocator::is_installed`
  and `largest_allocation_during`. `flui-foundation`'s `dhat` feature forwards it to DHAT's
  allocator.

### Changed

- `flui_testing::heap::{CountingAllocator, HeapCounts}` are re-exports of
  `flui_foundation::testing::heap`, and `flui-testing`'s `dhat` feature turns on
  `flui-foundation/dhat`. The allocator now forwards `realloc` and `alloc_zeroed` to the inner
  allocator instead of allocating, copying and freeing; it counts them as before.

### Removed

- The thirteen counting `GlobalAlloc`s of the tests and benches of flui-foundation,
  flui-animation, flui-engine, flui-interaction, flui-native, flui-painting, flui-rendering,
  flui-scheduler and flui-widgets: each installs `CountingAllocator`.
