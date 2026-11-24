### Added

- `PipelineCell::with_batched_removals` runs a closure during which `PipelineOwner::remove_render_object` queues each subtree, then removes them all and compacts each parent's child list once.

### Fixed

- A column, row or stack that drops many children in one update (a filtered list) no longer pays a scan and a shift of its render child list per removed child: dropping `k` of `n` children costs `O(n + k)` instead of `O(k·n)`.
