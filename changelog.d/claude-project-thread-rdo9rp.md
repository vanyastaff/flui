### Fixed

- **Lazy list items keep their children in order** (`flui-view`): inside a `ListView` (or any
  lazy sliver) item, a `Row`, `Column`, `Stack` or `Wrap` whose first child is a component
  (a `StatelessView`, or a `StatefulView` such as `PanClaimRegion`) laid that child out after its
  render sibling. The post-build pass that puts render children back into slot order now walks
  into a sliver's lazily built children too.
