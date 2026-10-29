### Changed

- A layout walk acquires the render nodes it reaches instead of borrowing the
  whole dirty subtree first, so a relayout that stops at cached children costs
  the nodes it touches, not the size of the subtree or the slab. The tree owns
  its slot storage (`slab` is no longer a dependency of `flui-rendering`).

### Added

- `PipelineCounters::nodes_acquired`: the nodes the layout walks entered,
  including those served from their caches.
