### Fixed

- **`flui-semantics`**: a merged node's standard action (tap, long press, …) runs the first
  handler in tree order, the merging node's own or else the first descendant's that defines it.
  It used to run the last descendant's, so a screen reader activating a `MergeSemantics` group
  could run a different control's command
  ([ADR-0206](/docs/adr/ADR-0206-typed-semantics-attributes.md)).
