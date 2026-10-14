### Fixed

- **Page-transition hooks** (`flui-widgets`): a route pushed before its navigator mounts, or
  while the navigator reads a builder's durations (a `duration` that pushes), now adopts the
  `PageTransitions` hook once it resolves instead of keeping the jump cut or adopting the
  outgoing hook. A panicking `Drop` of both the page and popup builder no longer aborts: the
  first failure stays authoritative and the second builder is retained.
