### Added

- **`flui-widgets`**: declarative `AnimatedList`, `AnimatedGrid` and `SliverAnimatedList`.
  Rebuild them with new items, a key function and an item builder; one keyed diff animates the
  difference. A new key enters through a `Transition` (default fade); a dropped key leaves from
  its last view, holding its index slot until its exit runs out, with nothing kept by the
  caller; a key returning mid-exit takes back the same element and state and turns back from
  the progress it reached; moved items slide into place (FLIP) along a `MotionSpec`, or
  `placement(None)` places them at once. `stagger(effects, stagger)` plays an `Effects`
  entrance on the items a list mounts with. Two items with one key panic with a `BUG:` message
  naming it. Every motion follows `MotionOverride::Reduce`.
- **`flui-objects`**: `Placement` and `PlacementSlot` — FLIP placement for
  `RenderSliverList` and `RenderSliverGrid` (`set_placement`), reached through the lazy
  adaptor's `SliverMultiBoxAdaptor::placement` and `LazyMultiBoxRender::set_placement`. A child
  slides only when the owner's roster generation changed and the child was laid out in both
  passes, so scrolling, an estimate correction or a resize places children at once.
