### Added

- **`flui-view`**: `KeepAliveHandle::pin` takes a hold whose lazy child is also laid out while it
  is outside the cache band: it keeps its state, stays in the accessibility tree at a current rect,
  and is neither painted nor hit-tested. `ListView`, `SliverFixedExtentList`, `SliverGrid` and
  `Carousel` items all honour it, and releasing the pin evicts the row on the next frame without a
  scroll
  ([ADR-0265](/docs/adr/ADR-0265-pinned-lazy-children-and-the-focusable-list.md)).
