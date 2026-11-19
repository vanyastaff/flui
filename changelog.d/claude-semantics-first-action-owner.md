### Fixed

- **`flui-semantics`**: a merged node's standard action (tap, long press, …) runs the merging
  node's own handler, or else the first descendant's in reading order (sort key, then tree
  order). It used to run the last descendant's, so activating a `MergeSemantics` group ran
  the last control in it rather than the first
  ([ADR-0206](/docs/adr/ADR-0206-typed-semantics-attributes.md)).
