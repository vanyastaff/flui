### Added

- `PipelineCell::with_batched_removals` runs a closure during which `PipelineOwner::remove_render_object` leaves each former parent's child list to the batch's end, which compacts every such list once.

### Fixed

- A column, row or stack that drops many children in one update (a filtered list) no longer pays a scan and a shift of its render child list per removed child: dropping `k` of `n` children costs `O(n + k)` instead of `O(k·n)`.
