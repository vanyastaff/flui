### Fixed

- Switching a `ClipRect`, `ClipRRect`, `ClipOval`, `ClipPath` or viewport between `Clip::None` and a
  clipping behavior now updates the semantics tree: content outside the bounds becomes hidden or
  visible to assistive technology in the same frame instead of keeping the previous flag.
